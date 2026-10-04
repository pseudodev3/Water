//! BNB public evidence is useful for observation, not complete wallet discovery.
use super::{
    model::*,
    providers::{hex, topic_address, valid_hash, Page, Providers},
};
use crate::model::Chain;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const FACTORY: &str = "0xca143ce32fe78f1f7019d7d551a6402fc5350c73";
const TRANSFER: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

pub async fn ensure_chain(p: &Providers) -> Result<(), String> {
    if p.evm_rpc(Chain::Bnb, "eth_chainId", json!([]))
        .await?
        .as_str()
        != Some("0x38")
    {
        return Err("BNB evidence rejected an RPC outside chain 56.".into());
    }
    Ok(())
}

pub async fn page(
    p: &Providers,
    candidate: &Candidate,
    cursor: Option<&str>,
) -> Result<Page, String> {
    ensure_chain(p).await?;
    if candidate.observed_tokens.is_empty() {
        return Ok(Page{records:vec![],cursor:cursor.map(str::to_string),exhausted:false,indexed:false,segments:vec![],scan_range:None,notes:vec!["BNB needs received token identities before public log collection can begin. Native-only and unobserved tokens are not indexed by this fallback.".into()]});
    }
    let block = p
        .evm_rpc(
            Chain::Bnb,
            "eth_getBlockByNumber",
            json!(["finalized", false]),
        )
        .await?;
    let head = hex(block["number"]
        .as_str()
        .ok_or("BNB finalized block missing.")?)?;
    let to = cursor
        .map(|v| {
            v.parse::<u64>()
                .map_err(|_| "BNB continuation was malformed.")
        })
        .transpose()?
        .unwrap_or(head)
        .min(head);
    let from = to.saturating_sub(999);
    let owner = &candidate.wallet[2..];
    let topic = format!("0x{owner:0>64}");
    let mut records = BTreeMap::new();
    for topics in [json!([TRANSFER, topic]), json!([TRANSFER, null, topic])] {
        let logs=p.evm_archive(Chain::Bnb,"eth_getLogs",json!([{"fromBlock":format!("0x{from:x}"),"toBlock":format!("0x{to:x}"),"address":candidate.observed_tokens,"topics":topics}])).await?;
        for log in logs
            .as_array()
            .ok_or("BNB transfer logs omitted an array.")?
        {
            if log["removed"].as_bool() == Some(true) {
                return Err("BNB finalized log was marked removed.".into());
            }
            let id = log["transactionHash"]
                .as_str()
                .filter(|s| valid_hash(s))
                .ok_or("BNB transfer omitted a transaction hash.")?
                .to_ascii_lowercase();
            let n = hex(log["blockNumber"]
                .as_str()
                .ok_or("BNB log block missing.")?)?;
            if n < from || n > to {
                return Err("BNB log escaped the requested block interval.".into());
            }
            records.entry(id.clone()).or_insert(Record{id,raw:json!({"block_number":n,"log":log,"scope":"Public ERC-20/NFT transfer reference; native-only transactions may be missing."}),transaction:None,error:None});
        }
    }
    let ids = records.keys().cloned().collect();
    let exhausted = from == 0;
    Ok(Page{records:records.into_values().collect(),cursor:(!exhausted).then(||(from-1).to_string()),exhausted,indexed:false,notes:vec!["BNB public logs cover received token identities only (up to 64). Other tokens, native-only, failed and internal-only transactions may be missing. Full 30/60-day qualification remains unavailable.".into()],segments:vec![(ids,exhausted)],scan_range:Some((from,to))})
}

pub fn valid_swap(log: &Value) -> bool {
    let Some(topics) = log["topics"].as_array() else {
        return false;
    };
    let Some(data) = log["data"].as_str() else {
        return false;
    };
    if topics.len() != 3
        || topics[0].as_str() != Some(super::venues::v2_topic().as_str())
        || data.len() != 258
        || !data.starts_with("0x")
        || !data[2..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return false;
    }
    if topics[1..]
        .iter()
        .any(|t| t.as_str().is_none_or(|s| topic_address(s).is_err()))
    {
        return false;
    }
    let positive: Vec<_> = data[2..]
        .as_bytes()
        .chunks_exact(64)
        .map(|w| w.iter().any(|b| *b != b'0'))
        .collect();
    positive == [true, false, false, true] || positive == [false, true, true, false]
}

pub async fn verified_pair(
    p: &Providers,
    log: &Value,
    block: &Value,
) -> Result<Option<(BTreeSet<String>, Value)>, String> {
    if !valid_swap(log) {
        return Ok(None);
    }
    let emitter = log["address"]
        .as_str()
        .ok_or("BNB swap emitter missing.")?
        .to_ascii_lowercase();
    if wallet_key(Chain::Bnb, &emitter).is_err() {
        return Err("BNB swap emitter was malformed.".into());
    }
    let code = p
        .evm_archive(Chain::Bnb, "eth_getCode", json!([emitter, block]))
        .await?;
    if !code
        .as_str()
        .is_some_and(super::venues::verified_bnb_v2_runtime)
    {
        return Ok(None);
    }
    let mut tokens = Vec::new();
    for selector in ["0x0dfe1681", "0xd21220a7"] {
        let token = p
            .evm_archive(
                Chain::Bnb,
                "eth_call",
                json!([{"to":emitter,"data":selector},block]),
            )
            .await?;
        tokens.push(topic_address(
            token.as_str().ok_or("BNB pool token missing.")?,
        )?);
    }
    if tokens[0] == tokens[1] {
        return Ok(None);
    }
    let factory = p
        .evm_archive(
            Chain::Bnb,
            "eth_call",
            json!([{"to":emitter,"data":"0xc45a0155"},block]),
        )
        .await?;
    if topic_address(factory.as_str().ok_or("BNB pool factory missing.")?)? != FACTORY {
        return Ok(None);
    }
    let pair=p.evm_archive(Chain::Bnb,"eth_call",json!([{"to":FACTORY,"data":format!("0xe6a43905{:0>64}{:0>64}",&tokens[0][2..],&tokens[1][2..])},block])).await?;
    if topic_address(pair.as_str().ok_or("BNB factory pair missing.")?)? != emitter {
        return Ok(None);
    }
    let assets = tokens
        .iter()
        .map(|t| {
            if t == "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c" {
                "BNB".into()
            } else {
                t.clone()
            }
        })
        .collect();
    Ok(Some((
        assets,
        json!({"pool":emitter,"factory":FACTORY,"tokens":tokens,"block":block,"runtime_template":"PancakeSwap V2"}),
    )))
}

pub async fn discover(p: &Providers, limit: usize) -> Result<Vec<Candidate>, String> {
    ensure_chain(p).await?;
    let block = p
        .evm_rpc(
            Chain::Bnb,
            "eth_getBlockByNumber",
            json!(["finalized", true]),
        )
        .await?;
    let mut out = Vec::new();
    for tx in block["transactions"]
        .as_array()
        .ok_or("BNB finalized transactions missing.")?
        .iter()
        .filter(|t| {
            t["to"].as_str().is_some()
                && t["input"].as_str().is_some_and(|s| s.len() > 10)
                && matches!(
                    t["type"].as_str(),
                    Some("0x0" | "0x1" | "0x2" | "0x3" | "0x4")
                )
        })
        .take(4)
    {
        let id = tx["hash"]
            .as_str()
            .filter(|s| valid_hash(s))
            .ok_or("BNB discovery hash missing.")?;
        let receipt = p
            .evm_rpc(Chain::Bnb, "eth_getTransactionReceipt", json!([id]))
            .await?;
        if receipt["status"].as_str() != Some("0x1")
            || receipt["transactionHash"].as_str() != Some(id)
            || tx["blockHash"] != receipt["blockHash"]
            || receipt["blockHash"] != block["hash"]
        {
            continue;
        }
        let mut verified = false;
        for log in receipt["logs"]
            .as_array()
            .ok_or("BNB discovery receipt logs missing.")?
            .iter()
            .filter(|l| valid_swap(l))
            .take(2)
        {
            if verified_pair(p, log, &tx["blockNumber"]).await?.is_some() {
                verified = true;
                break;
            }
        }
        if !verified {
            continue;
        }
        let wallet = wallet_key(
            Chain::Bnb,
            tx["from"].as_str().ok_or("BNB sender missing.")?,
        )?;
        if out.iter().any(|c: &Candidate| c.wallet == wallet) {
            continue;
        }
        let observed_tokens = receipt["logs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| {
                l["topics"][0].as_str() == Some(TRANSFER)
                    && l["topics"].as_array().is_some_and(|ts| ts.len() == 3)
                    && l["topics"].as_array().unwrap()[1..].iter().any(|t| {
                        t.as_str().is_some_and(|t| {
                            topic_address(t).ok().as_deref() == Some(wallet.as_str())
                        })
                    })
            })
            .filter_map(|l| l["address"].as_str())
            .filter_map(|a| wallet_key(Chain::Bnb, a).ok())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .take(64)
            .collect();
        out.push(Candidate{observed_tokens,chain:Chain::Bnb,wallet,discovered_at:now(),sources:vec![Source{name:"Onchain activity".into(),observed_at:hex(block["timestamp"].as_str().ok_or("BNB discovery timestamp missing.")?)?,detail:format!("Finalized successful PancakeSwap V2 execution {id}; factory membership and historical runtime verified. Sender is a research candidate; wallet ownership and economics require reconciliation."),profile:None}]});
        if out.len() >= (limit / 6).max(1) {
            break;
        }
    }
    Ok(out)
}
