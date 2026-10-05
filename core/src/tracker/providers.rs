use super::{budget::Lane, model::*, store::Store, venues};
use crate::{config::Config, model::Chain};
use num_bigint::BigUint;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
    sync::Arc,
};

const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const WRAPPED_SOL: &str = "So11111111111111111111111111111111111111112";
const TRANSFER: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

#[derive(Clone)]
pub struct Providers {
    pub http: reqwest::Client,
    pub config: Config,
    pub store: Arc<Store>,
    pub helius_keys: Vec<String>,
    pub helius_credit_limit: u64,
    pub fomo_key: Option<String>,
    pub rh_trace_url: String,
    pub bnb_trace_url: String,
    pub daily_limit: u64,
    pub lane: Option<Lane>,
}

pub struct Page {
    pub records: Vec<Record>,
    pub cursor: Option<String>,
    pub exhausted: bool,
    pub indexed: bool,
    pub notes: Vec<String>,
    pub segments: Vec<(Vec<String>, bool)>,
    pub scan_range: Option<(u64, u64)>,
}

pub fn record_timestamp(record: &Record) -> Option<u64> {
    record
        .transaction
        .as_ref()
        .map(|t| t.timestamp)
        .or_else(|| record.raw["blockTime"].as_u64())
        .or_else(|| {
            record.raw["timestamp"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .and_then(|d| u64::try_from(d.timestamp()).ok())
        })
}
pub fn record_priority(record: &Record) -> u64 {
    record_timestamp(record)
        .or_else(|| record.raw["block_number"].as_u64())
        .unwrap_or(0)
}

pub fn head_covered(page: &Page, known: &BTreeSet<String>) -> bool {
    page.segments
        .iter()
        .all(|(ids, exhausted)| *exhausted || ids.iter().any(|id| known.contains(id)))
}

pub fn range_covered(page: &Page, ranges: &[(u64, u64)]) -> bool {
    page.scan_range.is_some_and(|(from, to)| {
        ranges
            .iter()
            .any(|(a, b)| from <= b.saturating_add(1) && to.saturating_add(1) >= *a)
    })
}

pub fn add_scan_range(ranges: &mut Vec<(u64, u64)>, range: Option<(u64, u64)>) {
    if let Some(range) = range {
        ranges.push(range);
    }
    ranges.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for (a, b) in ranges.drain(..) {
        if let Some(last) = merged
            .last_mut()
            .filter(|last| a <= last.1.saturating_add(1))
        {
            last.1 = last.1.max(b);
        } else {
            merged.push((a, b));
        }
    }
    *ranges = merged;
}

impl Providers {
    fn evm_url(&self, chain: Chain) -> &str {
        match chain {
            Chain::Bnb => &self.config.bnb_rpc_url,
            _ => &self.config.robinhood_rpc_url,
        }
    }
    fn trace_url(&self, chain: Chain) -> &str {
        match chain {
            Chain::Bnb => &self.bnb_trace_url,
            _ => &self.rh_trace_url,
        }
    }
    pub(super) async fn evm_rpc(
        &self,
        chain: Chain,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        match self.rpc(self.evm_url(chain), method, params.clone()).await {
            Ok(value) => Ok(value),
            Err(primary) if matches!(chain, Chain::Bnb) => {
                let url = &self.config.bnb_fallback_rpc_url;
                if self.rpc(url, "eth_chainId", json!([])).await?.as_str() != Some("0x38") {
                    return Err("BNB fallback rejected an RPC outside chain 56.".into());
                }
                self.rpc(url, method, params).await.map_err(|second| {
                    format!("BNB public evidence unavailable: {primary} {second}")
                })
            }
            Err(error) => Err(error),
        }
    }
    pub fn reserve(&self) -> Result<(), String> {
        // One collector owns this counter. Persisting before sending makes the
        // daily ceiling survive process restarts and failed requests.
        self.store
            .reserve_http_in_lane(now(), self.daily_limit, None, self.lane)
    }

    pub async fn rpc(&self, url: &str, method: &str, params: Value) -> Result<Value, String> {
        if reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .as_deref()
            == Some("mainnet.helius-rpc.com")
        {
            self.store.reserve_work(
                now(),
                self.daily_limit,
                Some((self.helius_credit_limit, helius_cost(method, &params))),
                self.lane,
            )?;
        } else {
            self.reserve()?;
        }
        let response = self
            .http
            .post(url)
            .header(reqwest::header::USER_AGENT, "Water/0.1")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .map_err(|_| format!("{method} transport unavailable."))?;
        if !response.status().is_success() {
            return Err(format!(
                "{method} returned HTTP {}.",
                response.status().as_u16()
            ));
        }
        let value: Value = response
            .json()
            .await
            .map_err(|_| format!("{method} returned invalid JSON."))?;
        if value.get("error").is_some() {
            // Classify known failures without reflecting provider text, which
            // may contain credential-bearing URLs or other private context.
            let message = value["error"]["message"]
                .as_str()
                .unwrap_or("")
                .to_ascii_lowercase();
            let reason = if message.contains("historical state")
                || message.contains("missing trie node")
                || message.contains("state is not available")
            {
                " Historical state is unavailable."
            } else if message.contains("rate limit") || message.contains("quota") {
                " Provider rate or quota limit reached."
            } else if message.contains("execution reverted") {
                " Contract execution reverted."
            } else {
                ""
            };
            return Err(format!(
                "{method} failed (RPC code {}).{reason}",
                value["error"]["code"]
                    .as_i64()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".into())
            ));
        }
        value
            .get("result")
            .filter(|v| !v.is_null())
            .cloned()
            .ok_or_else(|| format!("{method} omitted its result."))
    }

    async fn helius_rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        let cooldown = self
            .store
            .state("helius-cooldown")?
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        if now() < cooldown {
            return Err("Helius collection is paused after a rate or quota response. Saved evidence remains available.".into());
        }
        if self.helius_keys.is_empty() {
            return Err("Helius credentials are not configured.".into());
        }
        let start = self
            .store
            .state("helius-active-key")?
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0)
            % self.helius_keys.len();
        for n in 0..self.helius_keys.len() {
            let index = (start + n) % self.helius_keys.len();
            let mut url = reqwest::Url::parse("https://mainnet.helius-rpc.com/").unwrap();
            url.query_pairs_mut()
                .append_pair("api-key", &self.helius_keys[index]);
            match self.rpc(url.as_str(), method, params.clone()).await {
                Ok(value) => {
                    self.store
                        .set_state("helius-active-key", &index.to_string())?;
                    return Ok(value);
                }
                Err(e) if e.contains("HTTP 401") || e.contains("HTTP 403") => continue,
                Err(e) => {
                    if e.contains("HTTP 429") {
                        self.store
                            .set_state("helius-cooldown", &(now() + 3600).to_string())?;
                    }
                    return Err(e);
                }
            }
        }
        Err("Helius rejected the configured credentials; saved history is retained.".into())
    }

    pub async fn token_metadata(
        &self,
        assets: &[String],
    ) -> Result<Vec<crate::model::TokenQuote>, String> {
        if self.helius_keys.is_empty() || assets.is_empty() {
            return Ok(Vec::new());
        }
        let value=self.helius_rpc("getAssetBatch",json!({"ids":assets.iter().take(1000).collect::<Vec<_>>(),"options":{"showFungible":true}})).await?;
        Ok(das_metadata(&value, assets, now()))
    }

    async fn solana_rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        if self.helius_keys.is_empty() {
            self.rpc(&self.config.solana_fallback_rpc_url, method, params)
                .await
        } else {
            // State, discovery and history share the same persisted budget and
            // cooldown. A quota response must not silently switch providers.
            self.helius_rpc(method, params).await
        }
    }

    async fn ensure_evm_chain(&self, chain: Chain, url: &str) -> Result<(), String> {
        let expected = chain.evm_chain_id().ok_or("Expected an EVM chain.")?;
        let value = self.rpc(url, "eth_chainId", json!([])).await?;
        if value.as_str().and_then(|v| hex(v).ok()) != Some(expected) {
            return Err(format!(
                "{} evidence rejected an RPC outside chain {expected}.",
                chain.label()
            ));
        }
        Ok(())
    }

    async fn finalized_evm_block(&self, chain: Chain) -> Result<Value, String> {
        let expected = chain.evm_chain_id().ok_or("Expected an EVM chain.")?;
        let id = self.evm_rpc(chain, "eth_chainId", json!([])).await?;
        if id.as_str().and_then(|v| hex(v).ok()) != Some(expected) {
            return Err(format!(
                "{} evidence rejected an RPC outside chain {expected}.",
                chain.label()
            ));
        }
        let block = self
            .evm_rpc(chain, "eth_getBlockByNumber", json!(["finalized", false]))
            .await?;
        hex(block["number"]
            .as_str()
            .ok_or("EVM finalized block number missing.")?)?;
        Ok(block)
    }

    async fn get(&self, url: reqwest::Url, key: Option<&str>) -> Result<Value, String> {
        self.reserve()?;
        let mut request = self
            .http
            .get(url)
            .header(reqwest::header::USER_AGENT, "Water/0.1");
        if let Some(key) = key {
            request = request.bearer_auth(key);
        }
        let response = request
            .send()
            .await
            .map_err(|_| "Wallet discovery/index transport unavailable.")?;
        if !response.status().is_success() {
            return Err(format!(
                "Wallet discovery/index returned HTTP {}.",
                response.status().as_u16()
            ));
        }
        response
            .json()
            .await
            .map_err(|_| "Wallet discovery/index returned invalid JSON.".into())
    }

    pub(super) async fn evm_archive(
        &self,
        chain: Chain,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        match self.evm_rpc(chain, method, params.clone()).await {
            Ok(value) => Ok(value),
            Err(primary) => {
                self.ensure_evm_chain(chain, self.trace_url(chain)).await?;
                self.rpc(self.trace_url(chain), method, params)
                    .await
                    .map_err(|secondary| {
                        format!("Historical EVM evidence unavailable: {primary} {secondary}")
                    })
            }
        }
    }

    pub async fn discover(&self, limit: usize) -> (Vec<Candidate>, Vec<String>) {
        let mut candidates = Vec::new();
        let mut notes = Vec::new();
        let timestamp = now();
        match self.get(reqwest::Url::parse("https://frontend-api-v3.pump.fun/pnl-leaderboard?period=monthly&sort=realized&limit=20").unwrap(),None).await {
            Ok(value)=>match pump_nominations(&value, (limit/3).max(1), timestamp) {
                Ok((rows,gaps))=>{candidates.extend(rows);notes.extend(gaps);},
                Err(e)=>notes.push(e),
            },Err(e)=>notes.push(format!("Pump discovery: {e}")),
        }
        notes.push("Pump monthly nominations cover Solana wallets. A permitted multichain social-wallet feed is not configured; RH and BNB use independent onchain discovery.".into());
        if let Some(key) = &self.fomo_key {
            match self
                .get(
                    reqwest::Url::parse("https://api.fomoapi.io/v2/leaderboard/30d").unwrap(),
                    Some(key),
                )
                .await
            {
                Ok(value) => {
                    // The independent API has its own explicit wallet fields.
                    // Never assign a profile/signer address as its execution account.
                    if let Some(rows) = value.get("traders").and_then(Value::as_array) {
                        for row in rows.iter().take((limit / 6).max(1)) {
                            for (field, chain) in
                                [("solana", Chain::Solana), ("evm", Chain::Robinhood)]
                            {
                                if let Some(address) = row["wallets"][field].as_str() {
                                    if let Ok(wallet) = wallet_key(chain, address) {
                                        candidates.push(Candidate{observed_tokens:Vec::new(),chain,wallet,discovered_at:timestamp,sources:vec![Source{name:"Fomo discovery".into(),observed_at:timestamp,detail:"Independent API-reported account association; app attribution and execution identity are not inferred from funding.".into(),profile:row["handle"].as_str().map(str::to_string)}]});
                                    }
                                }
                            }
                        }
                    } else {
                        notes.push(
                            "Fomo discovery omitted its documented traders/wallets structure."
                                .into(),
                        );
                    }
                }
                Err(e) => notes.push(format!("Fomo discovery: {e}")),
            }
        } else {
            notes.push("Fomo discovery has no configured permitted data access. Direct Fomo account polling is disabled.".into());
        }
        // A bounded recent finalized sample of supported SOL executions,
        // independent of leaderboard winners. Balance owners who signed the
        // execution are candidates; the fee payer alone does not prove ownership.
        for program in [
            "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
        ] {
            match self
                .solana_rpc(
                    "getSignaturesForAddress",
                    json!([program,{"limit":2,"commitment":"finalized"}]),
                )
                .await
            {
                Ok(rows) => {
                    if let Some(rows) = rows.as_array() {
                        for row in rows {
                            if !row.get("err").is_some_and(Value::is_null) {
                                continue;
                            }
                            let Some(id) = row["signature"].as_str() else {
                                continue;
                            };
                            let Ok(raw)=self.solana_rpc("getTransaction",json!([id,{"encoding":"jsonParsed","maxSupportedTransactionVersion":0,"commitment":"finalized"}])).await else{continue;};
                            let Some(keys) =
                                raw["transaction"]["message"]["accountKeys"].as_array()
                            else {
                                continue;
                            };
                            let mut found = false;
                            for signer in
                                keys.iter().filter(|k| k["signer"].as_bool() == Some(true))
                            {
                                let Some(address) = signer["pubkey"].as_str() else {
                                    continue;
                                };
                                let Ok(tx) = parse_solana(&raw, address, true) else {
                                    continue;
                                };
                                if tx.succeeded
                                    && tx.swap_evidence
                                    && tx.movement_complete
                                    && tx.assets.iter().any(|a| !quote(Chain::Solana, &a.asset))
                                    && tx.assets.iter().any(|a| quote(Chain::Solana, &a.asset))
                                {
                                    if let Ok(wallet) = wallet_key(Chain::Solana, address) {
                                        candidates.push(Candidate{observed_tokens:Vec::new(),chain:Chain::Solana,wallet,discovered_at:timestamp,sources:vec![Source{name:"Onchain activity".into(),observed_at:tx.timestamp,detail:format!("Recent finalized supported program sample: {program}; signed execution {id}. This is a candidate, not a profitability claim."),profile:None}]});
                                        found = true;
                                        break;
                                    }
                                }
                            }
                            if found {
                                break;
                            }
                        }
                    }
                }
                Err(error) => notes.push(format!("Solana onchain discovery: {error}")),
            }
        }
        // Bounded RH discovery uses successful V3 executions from verified
        // runtime emitters. ArbOS/system transactions are never trader seeds.
        if let Ok(chain) = self
            .rpc(&self.config.robinhood_rpc_url, "eth_chainId", json!([]))
            .await
        {
            if chain.as_str() == Some("0x1237") {
                if let Ok(block) = self
                    .rpc(
                        &self.config.robinhood_rpc_url,
                        "eth_getBlockByNumber",
                        json!(["finalized", true]),
                    )
                    .await
                {
                    let mut added = 0;
                    if let Some(rows) = block["transactions"].as_array() {
                        for row in rows
                            .iter()
                            .filter(|r| {
                                matches!(
                                    r["type"].as_str(),
                                    Some("0x0" | "0x1" | "0x2" | "0x3" | "0x4")
                                ) && r["input"].as_str().is_some_and(|v| v.len() > 10)
                            })
                            .take(8)
                        {
                            let (Some(id), Some(address)) =
                                (row["hash"].as_str(), row["from"].as_str())
                            else {
                                continue;
                            };
                            let Ok(receipt) = self
                                .rpc(
                                    &self.config.robinhood_rpc_url,
                                    "eth_getTransactionReceipt",
                                    json!([id]),
                                )
                                .await
                            else {
                                continue;
                            };
                            if receipt["status"].as_str() != Some("0x1")
                                || receipt["blockHash"] != block["hash"]
                            {
                                continue;
                            }
                            let mut verified = false;
                            if let Some(logs) = receipt["logs"].as_array() {
                                for log in logs.iter().filter(|l| {
                                    l["topics"][0].as_str() == Some(venues::v3_topic().as_str())
                                        && valid_v3_swap_log(l)
                                }) {
                                    let Some(emitter) = log["address"].as_str() else {
                                        continue;
                                    };
                                    if let Ok(code) = self
                                        .evm_archive(
                                            Chain::Robinhood,
                                            "eth_getCode",
                                            json!([emitter, row["blockNumber"]]),
                                        )
                                        .await
                                    {
                                        if code.as_str().is_some_and(venues::verified_v3_runtime) {
                                            verified = true;
                                            break;
                                        }
                                    }
                                }
                            }
                            if verified {
                                if let Ok(wallet) = wallet_key(Chain::Robinhood, address) {
                                    candidates.push(Candidate{observed_tokens:Vec::new(),chain:Chain::Robinhood,wallet,discovered_at:timestamp,sources:vec![Source{name:"Onchain activity".into(),observed_at:hex(block["timestamp"].as_str().unwrap_or("0x0")).unwrap_or(timestamp),detail:format!("Successful finalized V3 execution {id} from a verified pool runtime. Sender is a research candidate; execution ownership and profitability require wallet-history reconciliation."),profile:None}]});
                                    added += 1;
                                }
                            }
                            if added >= (limit / 6).max(1) {
                                break;
                            }
                        }
                    }
                }
            } else {
                notes.push("RH discovery rejected a non-4663 provider.".into());
            }
        }
        match super::bnb::discover(self, limit).await {
            Ok(rows) => candidates.extend(rows),
            Err(error) => notes.push(format!("BNB discovery: {error}")),
        }
        (candidates, notes)
    }

    pub async fn page(&self, candidate: &Candidate, cursor: Option<&str>) -> Result<Page, String> {
        match candidate.chain {
            Chain::Solana => {
                if !self.helius_keys.is_empty() {
                    let mut options = json!({"transactionDetails":"full","sortOrder":"desc","limit":100,"filters":{"tokenAccounts":"balanceChanged"}});
                    if let Some(cursor) = cursor {
                        options["paginationToken"] = json!(cursor);
                    }
                    let value = self
                        .helius_rpc(
                            "getTransactionsForAddress",
                            json!([candidate.wallet, options]),
                        )
                        .await?;
                    let mut page = helius_page(value, cursor, &candidate.wallet)?;
                    let finalized = self
                        .helius_rpc("getSlot", json!([{"commitment":"finalized"}]))
                        .await?
                        .as_u64()
                        .ok_or("Solana finalized slot missing.")?;
                    for record in &mut page.records {
                        if let Some(transaction) = &mut record.transaction {
                            transaction.finalized = transaction.block <= finalized;
                        }
                    }
                    return Ok(page);
                }
                let mut options = json!({"limit":100,"commitment":"finalized"});
                if let Some(cursor) = cursor {
                    options["before"] = json!(cursor);
                }
                let value = self
                    .rpc(
                        &self.config.solana_fallback_rpc_url,
                        "getSignaturesForAddress",
                        json!([candidate.wallet, options]),
                    )
                    .await?;
                let rows = value
                    .as_array()
                    .ok_or("Solana signature response was not an array.")?;
                let mut records = Vec::new();
                for row in rows {
                    let id = row["signature"]
                        .as_str()
                        .ok_or("Solana signature omitted an ID.")?;
                    records.push(Record {
                        id: id.into(),
                        raw: row.clone(),
                        transaction: None,
                        error: None,
                    });
                }
                let next = records.last().map(|r| r.id.clone());
                if next.as_deref() == cursor && next.is_some() {
                    return Err("Solana pagination repeated its cursor.".into());
                }
                let segments = vec![(
                    records.iter().map(|r| r.id.clone()).collect(),
                    rows.is_empty(),
                )];
                Ok(Page{records,cursor:next,exhausted:rows.is_empty(),indexed:false,notes:vec!["Public signature history cannot prove closed token-account or archive coverage; canonical transaction indices remain unresolved.".into()],segments,scan_range:None})
            }
            Chain::Bnb => super::bnb::page(self, candidate, cursor).await,
            Chain::Robinhood => {
                let chain = self
                    .rpc(&self.config.robinhood_rpc_url, "eth_chainId", json!([]))
                    .await?;
                if chain.as_str() != Some("0x1237") {
                    return Err("RH wallet history rejected a non-4663 provider.".into());
                }
                let mut states: BTreeMap<String, Option<Value>> = cursor
                    .map(serde_json::from_str)
                    .transpose()
                    .map_err(|_| "RH history cursor was malformed.")?
                    .unwrap_or_else(|| {
                        ["transactions", "token-transfers", "internal-transactions"]
                            .into_iter()
                            .map(|s| (s.into(), Some(json!({}))))
                            .collect()
                    });
                let mut records = BTreeMap::new();
                let mut segments = Vec::new();
                for route in ["transactions", "token-transfers", "internal-transactions"] {
                    let Some(Some(params)) = states.get(route).cloned() else {
                        continue;
                    };
                    let mut url = reqwest::Url::parse(&format!(
                        "{}/addresses/{}/{route}",
                        self.config.blockscout_api_url.trim_end_matches('/'),
                        candidate.wallet
                    ))
                    .map_err(|_| "RH history index URL was invalid.")?;
                    if let Some(key) = &self.config.blockscout_api_key {
                        url.query_pairs_mut().append_pair("apikey", key);
                    }
                    if let Some(object) = params.as_object() {
                        for (key, value) in object {
                            if value.is_string() || value.is_number() || value.is_boolean() {
                                url.query_pairs_mut().append_pair(
                                    key,
                                    value
                                        .as_str()
                                        .map(str::to_string)
                                        .unwrap_or_else(|| value.to_string())
                                        .as_str(),
                                );
                            }
                        }
                    }
                    let value = self.get(url, None).await?;
                    let rows = value["items"]
                        .as_array()
                        .ok_or("RH history index omitted items.")?;
                    let next = value
                        .get("next_page_params")
                        .ok_or("RH history index omitted explicit continuation.")?;
                    if rows.is_empty() && !next.is_null() {
                        return Err("RH history returned an empty page with continuation.".into());
                    }
                    if !next.is_null() && next == &params {
                        return Err("RH history pagination repeated its cursor.".into());
                    }
                    let mut ids = Vec::new();
                    for row in rows {
                        let id = row["transaction_hash"]
                            .as_str()
                            .or_else(|| row["hash"].as_str())
                            .ok_or("RH history row omitted transaction identity.")?;
                        if !valid_hash(id) {
                            return Err("RH history contained an invalid transaction hash.".into());
                        }
                        let id = id.to_ascii_lowercase();
                        ids.push(id.clone());
                        records.entry(id.clone()).or_insert(Record {
                            id,
                            raw: row.clone(),
                            transaction: None,
                            error: None,
                        });
                    }
                    states.insert(
                        route.into(),
                        if next.is_null() {
                            None
                        } else {
                            Some(next.clone())
                        },
                    );
                    segments.push((ids, next.is_null()));
                }
                let exhausted = states.values().all(Option::is_none);
                Ok(Page{records:records.into_values().collect(),cursor:Some(serde_json::to_string(&states).unwrap()),exhausted,indexed:true,notes:vec!["Three independent index routes cover transactions, token transfers and internal native transfers. Index completeness and ending balances still require reconciliation.".into()],segments,scan_range:None})
            }
        }
    }

    pub async fn fetch_record(
        &self,
        candidate: &Candidate,
        record: &Record,
    ) -> Result<Record, String> {
        match candidate.chain {
            Chain::Solana => {
                let params = json!([record.id,{"encoding":"jsonParsed","maxSupportedTransactionVersion":0,"commitment":"finalized"}]);
                let raw = self.solana_rpc("getTransaction", params).await?;
                let mut transaction = parse_solana(&raw, &candidate.wallet, true)?;
                if let Some(previous) = &record.transaction {
                    if previous.block == transaction.block {
                        transaction.index = previous.index;
                    }
                }
                Ok(Record {
                    id: record.id.clone(),
                    raw,
                    transaction: Some(transaction),
                    error: None,
                })
            }
            Chain::Robinhood | Chain::Bnb => {
                if matches!(candidate.chain, Chain::Bnb) {
                    super::bnb::ensure_chain(self).await?;
                }

                let tx = self
                    .evm_rpc(
                        candidate.chain,
                        "eth_getTransactionByHash",
                        json!([record.id]),
                    )
                    .await?;
                let receipt = self
                    .evm_rpc(
                        candidate.chain,
                        "eth_getTransactionReceipt",
                        json!([record.id]),
                    )
                    .await?;
                let block = self
                    .evm_rpc(
                        candidate.chain,
                        "eth_getBlockByNumber",
                        json!([tx["blockNumber"], false]),
                    )
                    .await?;
                let finalized = self
                    .evm_rpc(
                        candidate.chain,
                        "eth_getBlockByNumber",
                        json!(["finalized", false]),
                    )
                    .await?;
                let trace_access = self
                    .ensure_evm_chain(candidate.chain, self.trace_url(candidate.chain))
                    .await;
                if trace_access
                    .as_ref()
                    .is_err_and(|e| e.contains("outside chain"))
                {
                    return Err(trace_access.unwrap_err());
                }
                let trace_result = match trace_access {
                    Ok(_) => {
                        self.rpc(
                            self.trace_url(candidate.chain),
                            "debug_traceTransaction",
                            json!([record.id,{"tracer":"callTracer"}]),
                        )
                        .await
                    }
                    Err(error) => Err(error),
                };
                let trace_error = trace_result.as_ref().err().cloned();
                let trace = match trace_result {
                    Ok(value) => value,
                    Err(_) => Value::Null,
                };
                let mut decimals = BTreeMap::new();
                let mut verified_pairs = Vec::new();
                let mut verified_venues = Vec::new();
                let mut venue_errors = Vec::new();
                if matches!(candidate.chain, Chain::Bnb) {
                    for log in receipt["logs"]
                        .as_array()
                        .ok_or("BNB receipt logs missing.")?
                    {
                        match super::bnb::verified_pair(self, log, &tx["blockNumber"]).await {
                            Ok(Some((pair, evidence))) => {
                                verified_pairs.push(pair);
                                verified_venues.push(evidence);
                            }
                            Ok(None) => {}
                            Err(error) => venue_errors.push(error),
                        }
                    }
                }
                if matches!(candidate.chain, Chain::Robinhood) {
                    if let Some(logs) = receipt["logs"].as_array() {
                        for log in logs.iter().filter(|l| {
                            l["topics"][0].as_str() == Some(venues::v3_topic().as_str())
                        }) {
                            if valid_v3_swap_log(log) {
                                let emitter =
                                    log["address"].as_str().ok_or("Swap emitter missing.")?;
                                let runtime = self
                                    .evm_archive(
                                        candidate.chain,
                                        "eth_getCode",
                                        json!([emitter, tx["blockNumber"]]),
                                    )
                                    .await?;
                                if runtime.as_str().is_some_and(venues::verified_v3_runtime) {
                                    let mut pair = BTreeSet::new();
                                    for selector in ["0x0dfe1681", "0xd21220a7"] {
                                        let token=self.evm_archive(candidate.chain,"eth_call",json!([{"to":emitter,"data":selector},tx["blockNumber"]])).await?;
                                        let token = topic_address(
                                            token.as_str().ok_or("Pool token identity missing.")?,
                                        )?;
                                        pair.insert(if quote(Chain::Robinhood, &token) {
                                            "ETH".to_string()
                                        } else {
                                            token
                                        });
                                    }
                                    if pair.len() == 2 {
                                        verified_pairs.push(pair);
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(logs) = receipt["logs"].as_array() {
                    for log in logs
                        .iter()
                        .filter(|l| l["topics"][0].as_str() == Some(TRANSFER))
                    {
                        let from = topic_address(
                            log["topics"][1]
                                .as_str()
                                .ok_or("Transfer sender missing.")?,
                        )?;
                        let to = topic_address(
                            log["topics"][2]
                                .as_str()
                                .ok_or("Transfer receiver missing.")?,
                        )?;
                        if from != candidate.wallet && to != candidate.wallet {
                            continue;
                        }
                        if let Some(address) = log["address"].as_str() {
                            let address = address.to_ascii_lowercase();
                            if !decimals.contains_key(&address) {
                                let result=self.evm_archive(candidate.chain,"eth_call",json!([{"to":address,"data":"0x313ce567"},tx["blockNumber"]])).await;
                                let value = match result {
                                    Ok(value) => value,
                                    Err(_) => continue,
                                };
                                let value =
                                    hex(value.as_str().ok_or("Token decimals were not hex.")?)?;
                                let n =
                                    u32::try_from(value).map_err(|_| "Token decimals overflow.")?;
                                if n > 28 {
                                    return Err(
                                        "Token decimals exceed exact decimal precision.".into()
                                    );
                                }
                                decimals.insert(address, n);
                            }
                        }
                    }
                }
                let mut transaction = parse_evm(
                    candidate.chain,
                    &record.id,
                    &candidate.wallet,
                    &tx,
                    &receipt,
                    &trace,
                    &block,
                    &decimals,
                    hex(finalized["number"]
                        .as_str()
                        .ok_or("RH finalized block missing.")?)?,
                )?;
                let moved: BTreeSet<_> = transaction
                    .assets
                    .iter()
                    .filter(|a| a.quantity != Decimal::ZERO)
                    .map(|a| {
                        if a.asset.eq_ignore_ascii_case(native(candidate.chain))
                            || a.asset.eq_ignore_ascii_case(
                                if matches!(candidate.chain, Chain::Bnb) {
                                    "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c"
                                } else {
                                    "0x0bd7d308f8e1639fab988df18a8011f41eacad73"
                                },
                            )
                        {
                            native(candidate.chain).to_string()
                        } else {
                            a.asset.clone()
                        }
                    })
                    .collect();
                transaction.swap_evidence =
                    moved.len() == 2 && verified_pairs.iter().any(|pair| pair == &moved);
                if !venue_errors.is_empty() {
                    transaction.notes.push("Historical pool verification unavailable; received receipts remain observations and unverified events do not establish swaps.".into());
                }
                Ok(Record {
                    id: record.id.clone(),
                    raw: json!({"transaction":tx,"receipt":receipt,"trace":trace,"block":block,"decimals":decimals,"verified_venues":verified_venues,"venue_errors":venue_errors,"trace_error":trace_error}),
                    transaction: Some(transaction),
                    error: None,
                })
            }
        }
    }

    pub async fn execution_account(
        &self,
        candidate: &Candidate,
        paid_solana_fee: bool,
    ) -> Result<bool, String> {
        match candidate.chain {
            Chain::Solana => {
                let bytes = bs58::decode(&candidate.wallet)
                    .into_vec()
                    .map_err(|_| "Invalid Solana execution account.")?;
                let point: [u8; 32] = bytes
                    .try_into()
                    .map_err(|_| "Invalid Solana execution account length.")?;
                if curve25519_dalek::edwards::CompressedEdwardsY(point)
                    .decompress()
                    .is_none()
                {
                    return Ok(false);
                }
                let value = self
                    .solana_rpc(
                        "getAccountInfo",
                        json!([candidate.wallet,{"encoding":"base64","commitment":"finalized"}]),
                    )
                    .await?;
                Ok(
                    value["value"]["owner"].as_str() == Some("11111111111111111111111111111111")
                        || (value["value"].is_null() && paid_solana_fee),
                )
            }
            Chain::Robinhood | Chain::Bnb => {
                let block = self.finalized_evm_block(candidate.chain).await?;
                let code = self
                    .evm_archive(
                        candidate.chain,
                        "eth_getCode",
                        json!([candidate.wallet, block["number"]]),
                    )
                    .await?;
                Ok(code.as_str().is_some_and(standard_evm_account))
            }
        }
    }

    /// Bounded capital screen. EVM known-token reads do not prove a complete
    /// wallet inventory; a native or partial-token lower bound may still admit.
    pub async fn value_inventory(
        &self,
        candidate: &Candidate,
        assets: &BTreeSet<String>,
        quotes: &BTreeMap<String, crate::model::TokenQuote>,
        minimum: Decimal,
    ) -> Result<super::eligibility::Inventory, String> {
        use super::eligibility::{assess, Inventory};
        let at = now();
        let (quantity, block) = match candidate.chain {
            Chain::Solana => {
                let value = self
                    .solana_rpc(
                        "getBalance",
                        json!([candidate.wallet,{"commitment":"finalized"}]),
                    )
                    .await?;
                (
                    Decimal::from(
                        value["value"]
                            .as_u64()
                            .ok_or("Native Solana balance is missing.")?,
                    ) / Decimal::from(1_000_000_000u64),
                    format!(
                        "finalized native slot {}",
                        value["context"]["slot"]
                            .as_u64()
                            .ok_or("Native balance slot is missing.")?
                    ),
                )
            }
            Chain::Robinhood | Chain::Bnb => {
                let block = self.finalized_evm_block(candidate.chain).await?;
                let height = block["number"]
                    .as_str()
                    .ok_or("Native balance block is missing.")?;
                let value = self
                    .evm_rpc(
                        candidate.chain,
                        "eth_getBalance",
                        json!([candidate.wallet, height]),
                    )
                    .await?;
                (
                    scaled_hex(value.as_str().ok_or("Native EVM balance is missing.")?, 18)?,
                    height.into(),
                )
            }
        };
        let mut inventory = Inventory {
            quantities: BTreeMap::from([(native(candidate.chain).into(), quantity)]),
            observed_at: at,
            block,
            source: format!("{} finalized native/token RPC", candidate.chain.label()),
            complete: false,
            error: None,
        };
        // A proved lower bound above the floor needs no expensive token census.
        if assess(Some(&inventory), quotes, minimum, at, 0).status == "eligible" {
            return Ok(inventory);
        }
        match candidate.chain {
            Chain::Solana => {
                for program in [TOKEN_PROGRAM, TOKEN_2022] {
                    let value = self.solana_rpc("getTokenAccountsByOwner",json!([candidate.wallet,{"programId":program},{"encoding":"jsonParsed","commitment":"finalized"}])).await?;
                    inventory.block.push_str(&format!(
                        " / token slot {}",
                        value["context"]["slot"]
                            .as_u64()
                            .ok_or("Token balance slot is missing.")?
                    ));
                    for row in value["value"]
                        .as_array()
                        .ok_or("Token inventory is missing.")?
                    {
                        let info = &row["account"]["data"]["parsed"]["info"];
                        let mint = info["mint"]
                            .as_str()
                            .ok_or("Token inventory mint is missing.")?;
                        let quantity = token_amount(&info["tokenAmount"])?;
                        let held = inventory.quantities.entry(mint.into()).or_default();
                        *held = held
                            .checked_add(quantity)
                            .ok_or("Token inventory exceeds supported precision.")?;
                    }
                }
                inventory.complete = true;
            }
            Chain::Robinhood | Chain::Bnb => {
                for asset in assets
                    .iter()
                    .filter(|a| !a.eq_ignore_ascii_case(native(candidate.chain)))
                    .take(4)
                {
                    let read = async {
                            let value = self.evm_archive(candidate.chain,"eth_call",json!([{"to":asset,"data":format!("0x70a08231000000000000000000000000{}",&candidate.wallet[2..])},inventory.block])).await?;
                            let decimals = if let Some(d) = quotes.get(asset).and_then(|q|q.decimals) { d as u32 } else {
                                let d = self.evm_archive(candidate.chain,"eth_call",json!([{"to":asset,"data":"0x313ce567"},inventory.block])).await?;
                                u32::try_from(hex(d.as_str().ok_or("Token precision is missing.")?)?).map_err(|_|"Token precision is unsupported.")?
                            };
                            scaled_hex(value.as_str().ok_or("Token balance is missing.")?,decimals)
                        }.await;
                    if let Ok(quantity) = read {
                        inventory.quantities.insert(asset.clone(), quantity);
                    }
                }
            }
        }
        Ok(inventory)
    }

    pub async fn balances(
        &self,
        candidate: &Candidate,
        assets: &BTreeSet<String>,
    ) -> Result<BTreeMap<String, Decimal>, String> {
        self.balance_read(candidate, assets)
            .await
            .map(|(balances, _)| balances)
    }

    pub async fn balance_read(
        &self,
        candidate: &Candidate,
        assets: &BTreeSet<String>,
    ) -> Result<(BTreeMap<String, Decimal>, String), String> {
        let mut balances = BTreeMap::new();
        let block_reference;
        match candidate.chain {
            Chain::Solana => {
                let mut slots = Vec::new();
                for program in [TOKEN_PROGRAM, TOKEN_2022] {
                    let value=self.solana_rpc("getTokenAccountsByOwner",json!([candidate.wallet,{"programId":program},{"encoding":"jsonParsed","commitment":"finalized"}])).await?;
                    slots.push(
                        value["context"]["slot"]
                            .as_u64()
                            .ok_or("Solana balance response omitted its slot.")?,
                    );
                    let rows = value["value"]
                        .as_array()
                        .ok_or("Solana balances omitted value.")?;
                    for row in rows {
                        let info = &row["account"]["data"]["parsed"]["info"];
                        let mint = info["mint"].as_str().ok_or("Token balance omitted mint.")?;
                        if mint == WRAPPED_SOL {
                            continue;
                        }
                        let amount = token_amount(&info["tokenAmount"])?;
                        *balances.entry(mint.into()).or_default() += amount;
                    }
                }
                block_reference =
                    format!("finalized token-account slots {} / {}", slots[0], slots[1]);
            }
            Chain::Robinhood | Chain::Bnb => {
                let block = self.finalized_evm_block(candidate.chain).await?;
                block_reference = block["number"]
                    .as_str()
                    .ok_or("Finalized balance block omitted its number.")?
                    .to_string();
                for asset in assets.iter().filter(|a| !quote(candidate.chain, a)) {
                    let data = format!(
                        "0x70a08231000000000000000000000000{}",
                        &candidate.wallet[2..]
                    );
                    let value = self
                        .evm_archive(
                            candidate.chain,
                            "eth_call",
                            json!([{"to":asset,"data":data},block["number"]]),
                        )
                        .await?;
                    let decimals = self
                        .evm_archive(
                            candidate.chain,
                            "eth_call",
                            json!([{"to":asset,"data":"0x313ce567"},block["number"]]),
                        )
                        .await?;
                    balances.insert(
                        asset.clone(),
                        scaled_hex(
                            value.as_str().ok_or("EVM balance missing.")?,
                            u32::try_from(hex(decimals
                                .as_str()
                                .ok_or("EVM decimals missing.")?)?)
                            .map_err(|_| "EVM token precision exceeds the supported range.")?,
                        )?,
                    );
                }
            }
        }
        Ok((balances, block_reference))
    }
}

fn pump_nominations(
    value: &Value,
    limit: usize,
    timestamp: u64,
) -> Result<(Vec<Candidate>, Vec<String>), String> {
    let rows = value["entries"]
        .as_array()
        .ok_or("Pump discovery response omitted entries.")?;
    let mut candidates = Vec::new();
    let mut notes = Vec::new();
    for row in rows.iter().take(limit) {
        let Some(address) = row["walletAddress"].as_str() else {
            continue;
        };
        // This feed reports SOL amounts and a profile wallet. It does not
        // establish which RH/BNB execution account owns a multichain position.
        if address.starts_with("0x")
            || row["topPositions"].as_array().is_some_and(|positions| {
                positions
                    .iter()
                    .any(|p| p["chainId"].as_u64().is_some_and(|id| id != 1399811149))
            })
        {
            notes.push("Pump nomination lacks a verified execution-wallet/chain association; it was not assigned to RH or BNB.".into());
            continue;
        }
        if let Ok(wallet) = wallet_key(Chain::Solana, address) {
            candidates.push(Candidate {
                observed_tokens: Vec::new(), chain: Chain::Solana, wallet, discovered_at: timestamp,
                sources: vec![Source {
                    name: "Pump.fun".into(),
                    observed_at: row["lastRefreshedAtMs"].as_u64().unwrap_or(timestamp.saturating_mul(1000))/1000,
                    detail: "Monthly leaderboard nomination. Provider PnL is not Water qualification; chain activity and execution identity must be verified.".into(),
                    profile: row["username"].as_str().map(str::to_string),
                }],
            });
        }
    }
    Ok((candidates, notes))
}

pub fn helius_page(value: Value, previous: Option<&str>, wallet: &str) -> Result<Page, String> {
    let rows = value["data"]
        .as_array()
        .ok_or("Indexed Solana history omitted data.")?;
    let cursor = value
        .get("paginationToken")
        .ok_or("Indexed Solana history omitted continuation.")?;
    let next = if cursor.is_null() {
        None
    } else {
        Some(
            cursor
                .as_str()
                .ok_or("Indexed Solana cursor was not a string.")?
                .to_string(),
        )
    };
    if next.as_deref() == previous && next.is_some() {
        return Err("Indexed Solana pagination repeated its cursor.".into());
    }
    if rows.is_empty() && next.is_some() {
        return Err("Indexed Solana returned an empty page with continuation.".into());
    }
    let mut seen = BTreeSet::new();
    let mut records = Vec::new();
    for raw in rows {
        let transaction = parse_solana(raw, wallet, true)?;
        if !seen.insert(transaction.id.clone()) {
            return Err("Indexed Solana history repeated a transaction within a page.".into());
        }
        records.push(Record {
            id: transaction.id.clone(),
            raw: raw.clone(),
            transaction: Some(transaction),
            error: None,
        });
    }
    let segments = vec![(
        records.iter().map(|r| r.id.clone()).collect(),
        next.is_none(),
    )];
    Ok(Page{records,cursor:next.clone(),exhausted:next.is_none(),indexed:true,notes:vec!["Indexed token-account discovery before slot 111491819 is not complete without separate historical ownership evidence.".into()],segments,scan_range:None})
}

fn token_amount(value: &Value) -> Result<Decimal, String> {
    let amount = value["amount"]
        .as_str()
        .ok_or("Token amount missing exact integer.")?;
    let decimals = value["decimals"]
        .as_u64()
        .ok_or("Token decimals missing.")?;
    if decimals > 28 {
        return Err("Token amount exceeds exact supported precision.".into());
    }
    let raw = Decimal::from_str(amount)
        .map_err(|_| "Token integer amount exceeds supported precision.")?;
    Ok(raw
        / Decimal::from_str(&format!("1{}", "0".repeat(decimals as usize)))
            .map_err(|_| "Decimal precision overflow.")?)
}

pub fn parse_solana(raw: &Value, wallet: &str, finalized: bool) -> Result<Transaction, String> {
    let meta = raw
        .get("meta")
        .filter(|v| !v.is_null())
        .ok_or("Solana transaction omitted metadata.")?;
    let id = raw["transaction"]["signatures"][0]
        .as_str()
        .ok_or("Solana transaction omitted signature.")?
        .to_string();
    let timestamp = raw["blockTime"]
        .as_u64()
        .ok_or("Solana transaction omitted time.")?;
    let block = raw["slot"]
        .as_u64()
        .ok_or("Solana transaction omitted slot.")?;
    let mut keys: Vec<String> = raw["transaction"]["message"]["accountKeys"]
        .as_array()
        .ok_or("Solana account keys missing.")?
        .iter()
        .map(|v| {
            v.as_str()
                .or_else(|| v["pubkey"].as_str())
                .map(str::to_string)
                .ok_or_else(|| "Solana account key malformed.".to_string())
        })
        .collect::<Result<_, _>>()?;
    if raw["transaction"]["message"]["accountKeys"][0].is_string() {
        for kind in ["writable", "readonly"] {
            if let Some(loaded) = meta["loadedAddresses"][kind].as_array() {
                for key in loaded {
                    keys.push(key.as_str().ok_or("Loaded address malformed.")?.into());
                }
            }
        }
    }
    let pre = meta["preBalances"]
        .as_array()
        .ok_or("Solana pre-balances missing.")?;
    let post = meta["postBalances"]
        .as_array()
        .ok_or("Solana post-balances missing.")?;
    if pre.len() != keys.len() || post.len() != keys.len() {
        return Err("Solana balance/account cardinality mismatch.".into());
    }
    let mut deltas: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut owned = BTreeSet::new();
    let mut movement_complete = true;
    let mut foreign_new_accounts = false;
    for (field, sign) in [("preTokenBalances", -1i64), ("postTokenBalances", 1i64)] {
        let rows = meta[field]
            .as_array()
            .ok_or("Solana token balance metadata missing.")?;
        for row in rows {
            let owner = row["owner"].as_str();
            let index = row["accountIndex"]
                .as_u64()
                .ok_or("Token balance index missing.")? as usize;
            if index >= keys.len() {
                return Err("Token balance index exceeded account keys.".into());
            }
            if owner.is_none() {
                movement_complete = false;
                continue;
            }
            if owner == Some(wallet) {
                owned.insert(index);
                let mint = row["mint"].as_str().ok_or("Token mint missing.")?;
                if mint != WRAPPED_SOL {
                    *deltas.entry(mint.into()).or_default() +=
                        token_amount(&row["uiTokenAmount"])? * Decimal::from(sign);
                }
            } else if pre[index].as_u64() == Some(0) && post[index].as_u64().is_some_and(|v| v > 0)
            {
                foreign_new_accounts = true;
            }
        }
    }
    let wallet_index = keys.iter().position(|k| k == wallet);
    if let Some(index) = wallet_index {
        owned.insert(index);
    }
    let mut lamports = Decimal::ZERO;
    for index in owned {
        let a = pre[index].as_u64().ok_or("Native pre-balance malformed.")?;
        let b = post[index]
            .as_u64()
            .ok_or("Native post-balance malformed.")?;
        lamports += Decimal::from(b) - Decimal::from(a);
    }
    let paid = keys.first().is_some_and(|k| k == wallet);
    let fee = if paid {
        meta["fee"].as_u64().map(Decimal::from)
    } else {
        Some(Decimal::ZERO)
    };
    if let Some(fee) = fee {
        lamports += fee;
    }
    if lamports != Decimal::ZERO {
        deltas.insert("SOL".into(), lamports / Decimal::from(1_000_000_000u64));
    }
    let swap_evidence = solana_swap_logs(&meta["logMessages"]);
    let mut notes = Vec::new();
    if foreign_new_accounts && lamports != Decimal::ZERO {
        movement_complete = false;
        notes.push("Native flow may include rent for an account owned by another wallet.".into());
    }
    if !movement_complete {
        notes.push("Account ownership/rent effects require further reconstruction.".into());
    }
    let outcome = meta
        .get("err")
        .ok_or("Solana transaction outcome was missing.")?;
    Ok(Transaction {
        id,
        timestamp,
        block,
        index: raw["transactionIndex"].as_u64(),
        finalized,
        succeeded: outcome.is_null(),
        assets: deltas
            .into_iter()
            .filter(|(_, q)| *q != Decimal::ZERO)
            .map(|(asset, quantity)| Delta { asset, quantity })
            .collect(),
        fee_asset: "SOL".into(),
        fee_quantity: fee.map(|q| q / Decimal::from(1_000_000_000u64)),
        movement_complete,
        swap_evidence,
        notes,
        counterparties: solana_counterparties(raw, wallet),
    })
}

fn solana_counterparties(raw: &Value, wallet: &str) -> Vec<String> {
    let mut addresses = BTreeSet::new();
    let outer = raw["transaction"]["message"]["instructions"]
        .as_array()
        .into_iter()
        .flatten();
    let inner = raw["meta"]["innerInstructions"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|v| v["instructions"].as_array().into_iter().flatten());
    for instruction in outer.chain(inner) {
        if instruction["program"].as_str() != Some("system")
            || instruction["parsed"]["type"].as_str() != Some("transfer")
        {
            continue;
        }
        let info = &instruction["parsed"]["info"];
        if !info["lamports"].as_u64().is_some_and(|v| v > 0) {
            continue;
        }
        for (owner, other) in [("source", "destination"), ("destination", "source")] {
            if info[owner].as_str() == Some(wallet) {
                if let Some(address) = info[other].as_str() {
                    addresses.insert(address.to_string());
                }
            }
        }
    }
    addresses.into_iter().take(8).collect()
}

fn valid_v3_swap_log(log: &Value) -> bool {
    let Some(data) = log["data"].as_str().and_then(|s| s.strip_prefix("0x")) else {
        return false;
    };
    if !log["topics"].as_array().is_some_and(|t| t.len() == 3)
        || data.len() != 320
        || !data.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return false;
    }
    let (a, b) = (&data[..64], &data[64..128]);
    let negative = |s: &str| s.as_bytes()[0].to_ascii_lowercase() >= b'8';
    a.bytes().any(|b| b != b'0') && b.bytes().any(|b| b != b'0') && negative(a) != negative(b)
}

fn solana_swap_logs(logs: &Value) -> bool {
    let Some(rows) = logs.as_array() else {
        return false;
    };
    let mut stack = Vec::new();
    for row in rows {
        let Some(line) = row.as_str() else {
            continue;
        };
        let runtime = line
            .strip_prefix("Program ")
            .and_then(|s| s.split_once(' '))
            .filter(|(program, _)| wallet_key(Chain::Solana, program).is_ok());
        if let Some((program, event)) = runtime {
            if event.starts_with("invoke [") {
                stack.push(program);
            } else if (event == "success" || event.starts_with("failed:"))
                && stack.last().copied() == Some(program)
            {
                stack.pop();
            }
        } else if let Some(instruction) = line.strip_prefix("Program log: Instruction: ") {
            let supported = match stack.last().copied() {
                Some("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P") => {
                    matches!(instruction, "Buy" | "BuyExactSolIn" | "Sell")
                }
                Some("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4") => matches!(
                    instruction,
                    "Route"
                        | "RouteWithTokenLedger"
                        | "ExactOutRoute"
                        | "SharedAccountsRoute"
                        | "SharedAccountsRouteWithTokenLedger"
                        | "SharedAccountsExactOutRoute"
                ),
                _ => false,
            };
            if supported {
                return true;
            }
        }
    }
    false
}

fn native_counterparties(trace: &Value, wallet: &str, out: &mut BTreeSet<String>, reverted: bool) {
    let reverted = reverted || trace.get("error").is_some();
    if !reverted
        && matches!(
            trace["type"].as_str(),
            Some("CALL" | "CREATE" | "CREATE2" | "SELFDESTRUCT")
        )
        && trace["value"]
            .as_str()
            .is_some_and(|v| v != "0x0" && v != "0x00")
    {
        for (owner, other) in [("from", "to"), ("to", "from")] {
            if trace[owner]
                .as_str()
                .is_some_and(|a| a.eq_ignore_ascii_case(wallet))
            {
                if let Some(address) = trace[other].as_str() {
                    out.insert(address.to_ascii_lowercase());
                }
            }
        }
    }
    if let Some(calls) = trace["calls"].as_array() {
        for call in calls {
            native_counterparties(call, wallet, out, reverted);
        }
    }
}

pub fn native_calls(trace: &Value, wallet: &str) -> Result<Decimal, String> {
    fn walk(call: &Value, wallet: &str, reverted: bool, total: &mut Decimal) -> Result<(), String> {
        let reverted = reverted || call.get("error").is_some();
        let kind = call["type"].as_str().ok_or("Trace omitted call type.")?;
        if !reverted && matches!(kind, "CALL" | "CREATE" | "CREATE2" | "SELFDESTRUCT") {
            let value = scaled_hex(call["value"].as_str().unwrap_or("0x0"), 18)?;
            if call["from"]
                .as_str()
                .is_some_and(|a| a.eq_ignore_ascii_case(wallet))
            {
                *total -= value;
            }
            if call["to"]
                .as_str()
                .is_some_and(|a| a.eq_ignore_ascii_case(wallet))
            {
                *total += value;
            }
        }
        if let Some(calls) = call.get("calls") {
            for child in calls.as_array().ok_or("Trace child calls malformed.")? {
                walk(child, wallet, reverted, total)?;
            }
        }
        Ok(())
    }
    let mut total = Decimal::ZERO;
    walk(trace, wallet, false, &mut total)?;
    Ok(total)
}

fn arbitrum_fees_match(trace: &Value, wallet: &str, fee: Option<Decimal>) -> Result<bool, String> {
    if trace.get("beforeEVMTransfers").is_none() && trace.get("afterEVMTransfers").is_none() {
        return Ok(true);
    }
    let mut paid = Decimal::ZERO;
    for field in ["beforeEVMTransfers", "afterEVMTransfers"] {
        let Some(rows) = trace.get(field) else {
            continue;
        };
        for row in rows
            .as_array()
            .ok_or("Arbitrum system transfers malformed.")?
        {
            let quantity = scaled_hex(
                row["value"]
                    .as_str()
                    .ok_or("Arbitrum transfer amount missing.")?,
                18,
            )?;
            let from = row["from"]
                .as_str()
                .is_some_and(|a| a.eq_ignore_ascii_case(wallet));
            let to = row["to"]
                .as_str()
                .is_some_and(|a| a.eq_ignore_ascii_case(wallet));
            match row["purpose"].as_str() {
                Some("feePayment") if !to => {
                    if from {
                        paid += quantity;
                    }
                }
                Some("gasRefund") if !from => {
                    if to {
                        paid -= quantity;
                    }
                }
                Some("feeCollection") if !from && !to => {}
                _ => return Ok(false),
            }
        }
    }
    Ok(fee == Some(paid))
}

fn parse_evm(
    chain: Chain,
    id: &str,
    wallet: &str,
    tx: &Value,
    receipt: &Value,
    trace: &Value,
    block: &Value,
    decimals: &BTreeMap<String, u32>,
    finalized: u64,
) -> Result<Transaction, String> {
    if tx["hash"].as_str() != Some(id)
        || receipt["transactionHash"].as_str() != Some(id)
        || tx["blockHash"] != receipt["blockHash"]
        || receipt["blockHash"] != block["hash"]
        || tx["blockNumber"] != receipt["blockNumber"]
        || tx["blockNumber"] != block["number"]
    {
        return Err("RH transaction/receipt/block identity did not match.".into());
    }
    if !trace.is_null()
        && (trace["from"] != tx["from"]
            || trace["to"] != tx["to"]
            || scaled_hex(trace["value"].as_str().unwrap_or("0x0"), 18)?
                != scaled_hex(tx["value"].as_str().unwrap_or("0x0"), 18)?)
    {
        return Err("RH trace root did not match its transaction.".into());
    }
    let mut values = BTreeMap::new();
    let mut counterparties = BTreeSet::new();
    let mut complete = !trace.is_null()
        && matches!(
            tx["type"].as_str(),
            Some("0x0" | "0x1" | "0x2" | "0x3" | "0x4")
        );
    let succeeded = match receipt["status"].as_str() {
        Some("0x1") => true,
        Some("0x0") => false,
        _ => return Err("RH receipt omitted a valid outcome.".into()),
    };
    let swap_evidence = false; // Venue-specific event verification is a separate gate.
    for log in receipt["logs"]
        .as_array()
        .ok_or("RH receipt logs missing.")?
    {
        if log["topics"][0].as_str() != Some(TRANSFER) {
            continue;
        }
        let from = topic_address(
            log["topics"][1]
                .as_str()
                .ok_or("Transfer sender missing.")?,
        )?;
        let to = topic_address(
            log["topics"][2]
                .as_str()
                .ok_or("Transfer receiver missing.")?,
        )?;
        if from != wallet && to != wallet {
            continue;
        }
        if !log["topics"]
            .as_array()
            .is_some_and(|topics| topics.len() == 3)
        {
            return Err("Wallet transfer is not a supported ERC-20 event.".into());
        }
        let asset = log["address"]
            .as_str()
            .ok_or("Transfer contract missing.")?
            .to_ascii_lowercase();
        let Some(decimals) = decimals.get(&asset) else {
            complete = false;
            continue;
        };
        let amount = scaled_hex(
            log["data"].as_str().ok_or("Transfer amount missing.")?,
            *decimals,
        )?;
        if from == wallet {
            *values.entry(asset.clone()).or_default() -= amount;
            counterparties.insert(to.clone());
        }
        if to == wallet {
            *values.entry(asset).or_default() += amount;
            counterparties.insert(from);
        }
    }
    let native = if trace.is_null() {
        let mut outer = Decimal::ZERO;
        if succeeded {
            let amount = scaled_hex(
                tx["value"]
                    .as_str()
                    .ok_or("External native value missing.")?,
                18,
            )?;
            for (field, sign, other) in
                [("from", -Decimal::ONE, "to"), ("to", Decimal::ONE, "from")]
            {
                if tx[field]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case(wallet))
                {
                    outer += amount * sign;
                    if amount > Decimal::ZERO {
                        if let Some(a) = tx[other].as_str() {
                            counterparties.insert(a.to_ascii_lowercase());
                        }
                    }
                }
            }
        }
        outer
    } else {
        native_calls(trace, wallet)?
    };
    if !trace.is_null() {
        native_counterparties(trace, wallet, &mut counterparties, false);
    }
    if native != Decimal::ZERO {
        values.insert(super::model::native(chain).into(), native);
    }
    let sender = tx["from"]
        .as_str()
        .is_some_and(|a| a.eq_ignore_ascii_case(wallet));
    let fee = if sender {
        Some(
            scaled_hex(receipt["gasUsed"].as_str().ok_or("Gas used missing.")?, 0)?
                * scaled_hex(
                    receipt["effectiveGasPrice"]
                        .as_str()
                        .ok_or("Gas price missing.")?,
                    18,
                )?,
        )
    } else {
        complete = false;
        None
    };
    if matches!(chain, Chain::Robinhood) && !arbitrum_fees_match(trace, wallet, fee)? {
        complete = false;
    }
    if matches!(chain, Chain::Bnb) && fee == Some(Decimal::ZERO) {
        complete = false;
    }
    let number = hex(tx["blockNumber"]
        .as_str()
        .ok_or("RH block number missing.")?)?;
    Ok(Transaction {
        id: id.into(),
        timestamp: hex(block["timestamp"]
            .as_str()
            .ok_or("RH block time missing.")?)?,
        block: number,
        index: Some(hex(tx["transactionIndex"]
            .as_str()
            .ok_or("RH transaction index missing.")?)?),
        finalized: number <= finalized,
        succeeded,
        assets: values
            .into_iter()
            .filter(|(_, v)| *v != Decimal::ZERO)
            .map(|(asset, quantity)| Delta { asset, quantity })
            .collect(),
        fee_asset: super::model::native(chain).into(),
        fee_quantity: fee,
        movement_complete: complete,
        swap_evidence,
        notes: if trace.is_null() {
            vec!["Native call trace unavailable. Only external native value and receipt token deltas are observed; internal calls and complete economics remain unknown.".into()]
        } else if !complete {
            vec!["Token precision, account fee attribution or chain-specific system transfers require additional evidence.".into()]
        } else {
            vec![]
        },
        counterparties: counterparties.into_iter().take(8).collect(),
    })
}

pub(super) fn hex(value: &str) -> Result<u64, String> {
    u64::from_str_radix(value.strip_prefix("0x").ok_or("Expected 0x integer.")?, 16)
        .map_err(|_| "Hex integer exceeded supported precision.".into())
}
fn scaled_hex(value: &str, decimals: u32) -> Result<Decimal, String> {
    if decimals > 28 {
        return Err("Decimal precision exceeds support.".into());
    }
    let value = BigUint::parse_bytes(
        value
            .strip_prefix("0x")
            .ok_or("Expected 0x amount.")?
            .as_bytes(),
        16,
    )
    .ok_or("Invalid hex amount.")?;
    token_amount(&json!({"amount":value.to_str_radix(10),"decimals":decimals}))
}
pub(super) fn topic_address(value: &str) -> Result<String, String> {
    if value.len() != 66
        || !value.starts_with("0x")
        || !value[2..].chars().all(|c| c.is_ascii_hexdigit())
        || value[2..26].bytes().any(|b| b != b'0')
    {
        return Err("Transfer topic address malformed.".into());
    }
    Ok(format!("0x{}", &value[26..]).to_ascii_lowercase())
}
pub(super) fn valid_hash(value: &str) -> bool {
    value.len() == 66
        && value.starts_with("0x")
        && value[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn standard_evm_account(code: &str) -> bool {
    code == "0x"
        || (code.len() == 48
            && code.starts_with("0xef0100")
            && code[8..].bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RpcHarness {
        providers: Providers,
        requests: Arc<std::sync::Mutex<Vec<(String, Value)>>>,
        task: tokio::task::JoinHandle<()>,
        url: String,
    }

    impl Drop for RpcHarness {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn rpc_harness() -> RpcHarness {
        use axum::{
            extract::{Path, State},
            routing::post,
            Json, Router,
        };
        type Requests = Arc<std::sync::Mutex<Vec<(String, Value)>>>;
        async fn handler(
            State(requests): State<Requests>,
            Path(route): Path<String>,
            Json(request): Json<Value>,
        ) -> Json<Value> {
            requests
                .lock()
                .unwrap()
                .push((route.clone(), request.clone()));
            let method = request["method"].as_str().unwrap();
            let error = || {
                Json(
                    json!({"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"historical state is not available; https://example.test/?api-key=synthetic-secret"}}),
                )
            };
            let result = match (route.as_str(), method) {
                ("wrong" | "bnb", "eth_chainId") => json!("0x38"),
                ("primary" | "archive" | "noheight", "eth_chainId") => json!("0x1237"),
                ("primary" | "bnb", "eth_getBlockByNumber") => json!({"number":"0x4c18eda"}),
                ("noheight", "eth_getBlockByNumber") => json!({"number":null}),
                ("archive" | "bnb", "eth_getCode" | "eth_call") => {
                    if request["params"][1] != "0x4c18eda" {
                        return error();
                    }
                    if method == "eth_getCode" {
                        json!("0x")
                    } else if request["params"][0]["data"] == "0x313ce567" {
                        json!("0x12")
                    } else {
                        json!("0x2a")
                    }
                }
                ("solana", "getAccountInfo") => {
                    json!({"context":{"slot":1},"value":{"owner":"11111111111111111111111111111111"}})
                }
                ("solana", "getTokenAccountsByOwner") => json!({"context":{"slot":1},"value":[]}),
                _ => return error(),
            };
            Json(json!({"jsonrpc":"2.0","id":1,"result":result}))
        }
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/{route}", post(handler))
            .with_state(requests.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let primary = format!("{url}/primary");
        RpcHarness {
            providers: Providers {
                http: reqwest::Client::builder().no_proxy().build().unwrap(),
                config: Config {
                    port: 0,
                    gecko_api_host: primary.clone(),
                    dexscreener_api_host: primary.clone(),
                    solana_rpc_url: format!("{url}/solana"),
                    solana_fallback_rpc_url: format!("{url}/solana"),
                    robinhood_rpc_url: primary.clone(),
                    bnb_rpc_url: primary.clone(),
                    bnb_fallback_rpc_url: primary.clone(),
                    blockscout_api_url: primary,
                    blockscout_api_key: None,
                },
                store: Arc::new(Store::open(":memory:").unwrap()),
                helius_keys: vec![],
                helius_credit_limit: 800_000,
                fomo_key: None,
                rh_trace_url: format!("{url}/archive"),
                bnb_trace_url: format!("{url}/archive"),
                daily_limit: 100,
                lane: None,
            },
            requests,
            task,
            url,
        }
    }

    fn rh_candidate() -> Candidate {
        Candidate {
            chain: Chain::Robinhood,
            wallet: "0x4c64bac8ac8c9e091d0bd7c592a65d06dcf2f88b".into(),
            discovered_at: 0,
            sources: vec![],
            observed_tokens: vec![],
        }
    }

    fn sol_candidate() -> Candidate {
        Candidate {
            chain: Chain::Solana,
            wallet: "3jWTgYPG5s7WfaRvppPBXio4hHQxg18fLUkV2z5covSQ".into(),
            discovered_at: 0,
            sources: vec![],
            observed_tokens: vec![],
        }
    }

    fn rh_assets() -> BTreeSet<String> {
        BTreeSet::from(["0x2267629f4953a581c250cd01872f5e38990cd999".into()])
    }

    #[test]
    fn pump_board_nominations_do_not_guess_an_evm_chain_or_execution_account_from_a_profile() {
        let sol = sol_candidate().wallet;
        let board = json!({"entries":[
            {"walletAddress":sol,"username":"test-profile","pnlUsd":999999,"topPositions":[{"chainId":1399811149}]},
            {"walletAddress":rh_candidate().wallet,"pnlUsd":999999},
            {"walletAddress":sol,"topPositions":[{"chainId":56}]},
            {"walletAddress":sol,"topPositions":[{"chainId":4663}]}
        ]});
        let (rows, notes) = pump_nominations(&board, 20, 100).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].chain.key(), "solana");
        assert!(rows[0].observed_tokens.is_empty());
        assert_eq!(notes.len(), 3);
        assert!(pump_nominations(&json!({"error":"test upstream failure"}), 20, 100).is_err());
    }

    #[tokio::test]
    async fn rh_state_checks_recover_from_pruned_primary_without_changing_the_finalized_height() {
        let h = rpc_harness().await;
        assert!(h
            .providers
            .execution_account(&rh_candidate(), false)
            .await
            .unwrap());
        let balances = h
            .providers
            .balances(&rh_candidate(), &rh_assets())
            .await
            .unwrap();
        assert_eq!(
            balances.values().copied().collect::<Vec<_>>(),
            vec![Decimal::new(42, 18)]
        );
        let requests = h.requests.lock().unwrap();
        for (_, request) in requests
            .iter()
            .filter(|(_, r)| matches!(r["method"].as_str(), Some("eth_getCode" | "eth_call")))
        {
            assert_eq!(request["params"][1], "0x4c18eda");
        }
        assert!(requests
            .iter()
            .any(|(route, r)| route == "archive" && r["method"] == "eth_call"));
    }

    #[tokio::test]
    async fn rh_state_checks_reject_wrong_primary_and_archive_chains() {
        let mut h = rpc_harness().await;
        h.providers.rh_trace_url = format!("{}/wrong", h.url);
        assert!(h
            .providers
            .execution_account(&rh_candidate(), false)
            .await
            .unwrap_err()
            .contains("outside chain 4663"));
        assert!(h
            .providers
            .balances(&rh_candidate(), &rh_assets())
            .await
            .unwrap_err()
            .contains("outside chain 4663"));
        assert!(h
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(route, _)| route == "wrong")
            .all(|(_, r)| r["method"] == "eth_chainId"));
        h.providers.config.robinhood_rpc_url = format!("{}/wrong", h.url);
        assert!(h
            .providers
            .execution_account(&rh_candidate(), false)
            .await
            .unwrap_err()
            .contains("outside chain 4663"));
    }

    #[tokio::test]
    async fn bnb_state_checks_retain_chain_56_failover_when_the_primary_is_unavailable() {
        let mut h = rpc_harness().await;
        h.providers.config.bnb_rpc_url = format!("{}/offline", h.url);
        h.providers.config.bnb_fallback_rpc_url = format!("{}/bnb", h.url);
        let mut candidate = rh_candidate();
        candidate.chain = Chain::Bnb;
        assert!(h
            .providers
            .execution_account(&candidate, false)
            .await
            .unwrap());
        let balances = h
            .providers
            .balances(&candidate, &rh_assets())
            .await
            .unwrap();
        assert_eq!(
            balances.values().copied().collect::<Vec<_>>(),
            vec![Decimal::new(42, 18)]
        );
        assert!(!h
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|(route, _)| route == "archive"));
    }

    #[tokio::test]
    async fn unavailable_archive_keeps_state_unknown_and_sanitizes_rpc_errors() {
        let mut h = rpc_harness().await;
        // Same chain, but every archive state call fails like the primary.
        h.providers.rh_trace_url = format!("{}/primary", h.url);
        let account_error = h
            .providers
            .execution_account(&rh_candidate(), false)
            .await
            .unwrap_err();
        let balance_error = h
            .providers
            .balances(&rh_candidate(), &rh_assets())
            .await
            .unwrap_err();
        for error in [account_error, balance_error] {
            assert!(error.contains("Historical state is unavailable"));
            assert!(!error.contains("synthetic-secret") && !error.contains("example.test"));
        }
    }

    #[tokio::test]
    async fn evm_state_checks_require_a_received_finalized_height() {
        let mut h = rpc_harness().await;
        h.providers.config.robinhood_rpc_url = format!("{}/noheight", h.url);
        assert!(h
            .providers
            .execution_account(&rh_candidate(), false)
            .await
            .unwrap_err()
            .contains("block number missing"));
        assert!(h
            .providers
            .balances(&rh_candidate(), &rh_assets())
            .await
            .unwrap_err()
            .contains("block number missing"));
        assert!(!h
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|(_, r)| matches!(r["method"].as_str(), Some("eth_getCode" | "eth_call"))));
    }

    #[tokio::test]
    async fn configured_helius_routes_solana_state_checks_through_the_shared_pause_and_credit_budget(
    ) {
        let mut h = rpc_harness().await;
        h.providers.helius_keys = vec!["synthetic-key-one".into(), "synthetic-key-two".into()];
        h.providers
            .store
            .set_state("helius-cooldown", &(now() + 3600).to_string())
            .unwrap();
        assert!(h
            .providers
            .execution_account(&sol_candidate(), false)
            .await
            .unwrap_err()
            .contains("paused"));
        assert!(h
            .providers
            .balances(&sol_candidate(), &BTreeSet::new())
            .await
            .unwrap_err()
            .contains("paused"));
        h.providers.store.set_state("helius-cooldown", "0").unwrap();
        h.providers.helius_credit_limit = 10;
        h.providers
            .store
            .reserve_http(now(), 100, Some(10))
            .unwrap();
        assert!(h
            .providers
            .execution_account(&sol_candidate(), false)
            .await
            .unwrap_err()
            .contains("credit budget"));
        assert!(h
            .providers
            .balances(&sol_candidate(), &BTreeSet::new())
            .await
            .unwrap_err()
            .contains("credit budget"));
        assert_eq!(h.providers.store.helius_credits(now()).unwrap(), 10);
        assert!(
            h.requests.lock().unwrap().is_empty(),
            "Configured Helius must not be bypassed by public state reads or quota failover."
        );
    }

    #[tokio::test]
    async fn solana_state_checks_keep_the_public_fallback_when_helius_is_not_configured() {
        let h = rpc_harness().await;
        assert!(h
            .providers
            .execution_account(&sol_candidate(), false)
            .await
            .unwrap());
        assert!(h
            .providers
            .balances(&sol_candidate(), &BTreeSet::new())
            .await
            .unwrap()
            .is_empty());
        let requests = h.requests.lock().unwrap();
        let balance_requests: Vec<_> = requests
            .iter()
            .filter(|(_, r)| r["method"] == "getTokenAccountsByOwner")
            .collect();
        assert_eq!(balance_requests.len(), 2);
        assert_ne!(
            balance_requests[0].1["params"][1],
            balance_requests[1].1["params"][1]
        );
        assert!(balance_requests
            .iter()
            .all(|(_, r)| r["params"][2]["commitment"] == "finalized"));
    }

    #[tokio::test]
    async fn bnb_fetch_keeps_received_receipt_when_archive_verification_is_unavailable() {
        use axum::{routing::post, Json, Router};
        async fn rpc_stub(Json(request): Json<Value>) -> Json<Value> {
            let evidence: Value = serde_json::from_str(include_str!(
                "../../tests/fixtures/tracker-bnb-observed.json"
            ))
            .unwrap();
            let result = match request["method"].as_str().unwrap() {
                "eth_chainId" => json!("0x38"),
                "eth_getTransactionByHash" => evidence["transaction"].clone(),
                "eth_getTransactionReceipt" => evidence["receipt"].clone(),
                "eth_getBlockByNumber" => evidence["block"].clone(),
                _ => {
                    return Json(
                        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"Test: archive unavailable"}}),
                    )
                }
            };
            Json(json!({"jsonrpc":"2.0","id":1,"result":result}))
        }
        async fn wrong_chain() -> Json<Value> {
            Json(json!({"jsonrpc":"2.0","id":1,"result":"0x1237"}))
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/", post(rpc_stub))
                    .route("/wrong", post(wrong_chain)),
            )
            .await
            .unwrap()
        });
        let config = Config {
            port: 0,
            gecko_api_host: url.clone(),
            dexscreener_api_host: url.clone(),
            solana_rpc_url: url.clone(),
            solana_fallback_rpc_url: url.clone(),
            robinhood_rpc_url: url.clone(),
            bnb_rpc_url: url.clone(),
            bnb_fallback_rpc_url: url.clone(),
            blockscout_api_url: url.clone(),
            blockscout_api_key: None,
        };
        let mut providers = Providers {
            http: reqwest::Client::new(),
            config,
            store: Arc::new(Store::open(":memory:").unwrap()),
            helius_keys: vec![],
            helius_credit_limit: 800_000,
            fomo_key: None,
            rh_trace_url: url.clone(),
            bnb_trace_url: url.clone(),
            daily_limit: 100,
            lane: None,
        };
        let evidence: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/tracker-bnb-observed.json"
        ))
        .unwrap();
        let candidate = Candidate {
            chain: Chain::Bnb,
            wallet: evidence["wallet"].as_str().unwrap().into(),
            discovered_at: 0,
            sources: vec![],
            observed_tokens: vec![],
        };
        let reference = Record {
            id: evidence["id"].as_str().unwrap().into(),
            raw: Value::Null,
            transaction: None,
            error: None,
        };
        let record = providers
            .fetch_record(&candidate, &reference)
            .await
            .unwrap();
        assert_eq!(record.raw["receipt"], evidence["receipt"]);
        assert!(!record.raw["venue_errors"].as_array().unwrap().is_empty());
        assert!(record.raw["trace"].is_null());
        let transaction = record.transaction.unwrap();
        assert_eq!(
            transaction.fee_quantity,
            Some(Decimal::from_str("0.00001217909").unwrap())
        );
        assert!(!transaction.movement_complete && !transaction.swap_evidence);
        assert!(transaction.assets.is_empty());
        providers.bnb_trace_url = format!("{url}/wrong");
        assert!(providers
            .fetch_record(&candidate, &reference)
            .await
            .unwrap_err()
            .contains("outside chain 56"));
        task.abort();
    }
    #[test]
    fn robinhood_missing_trace_or_decimals_retains_fees_without_inventing_a_trade_price() {
        let v: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/tracker-rh-native-swap.json"
        ))
        .unwrap();
        let decimals = serde_json::from_value(v["decimals"].clone()).unwrap();
        let parsed = parse_evm(
            Chain::Robinhood,
            v["id"].as_str().unwrap(),
            v["wallet"].as_str().unwrap(),
            &v["transaction"],
            &v["receipt"],
            &Value::Null,
            &v["block"],
            &decimals,
            u64::MAX,
        )
        .unwrap();
        assert!(!parsed.movement_complete && !parsed.swap_evidence);
        assert!(parsed.fee_quantity.is_some());
        assert!(!parsed.assets.is_empty());
        let no_units = parse_evm(
            Chain::Robinhood,
            v["id"].as_str().unwrap(),
            v["wallet"].as_str().unwrap(),
            &v["transaction"],
            &v["receipt"],
            &Value::Null,
            &v["block"],
            &BTreeMap::new(),
            u64::MAX,
        )
        .unwrap();
        assert!(!no_units.movement_complete);
        assert!(no_units.assets.iter().all(|d| d.asset == "ETH"));
    }
    #[test]
    fn bnb_missing_trace_retains_receipt_evidence_and_exact_bnb_fee_without_complete_economics() {
        let v: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/tracker-bnb-observed.json"
        ))
        .unwrap();
        let decimals = serde_json::from_value(v["decimals"].clone()).unwrap();
        let tx = parse_evm(
            Chain::Bnb,
            v["id"].as_str().unwrap(),
            v["wallet"].as_str().unwrap(),
            &v["transaction"],
            &v["receipt"],
            &Value::Null,
            &v["block"],
            &decimals,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(tx.fee_asset, "BNB");
        assert_eq!(
            tx.fee_quantity,
            Some(Decimal::from_str("0.00001217909").unwrap())
        );
        assert!(!tx.movement_complete && !tx.swap_evidence);
        assert!(tx.assets.iter().all(|a| a.asset != "ETH"));
        assert!(tx.assets.is_empty(),"Unknown token denominations cannot become normalized quantities or a synthetic native proceeds leg.");
        assert!(tx.notes.iter().any(|n| n.contains("trace unavailable")));
    }
    #[test]
    fn public_empty_block_intervals_bridge_gaps_without_becoming_full_wallet_history() {
        let mut ranges = vec![(1000, 1999)];
        let page = Page {
            records: vec![],
            cursor: Some("2999".into()),
            exhausted: false,
            indexed: false,
            notes: vec![],
            segments: vec![],
            scan_range: Some((3000, 3999)),
        };
        assert!(!range_covered(&page, &ranges));
        add_scan_range(&mut ranges, Some((2000, 2999)));
        assert!(range_covered(&page, &ranges));
        add_scan_range(&mut ranges, page.scan_range);
        assert_eq!(ranges, vec![(1000, 3999)]);
        assert!(!page.indexed);
    }
    #[test]
    fn short_page_with_continuation_and_application_errors_are_not_completion() {
        assert!(helius_page(json!({"data":[],"paginationToken":"more"}), None, "wallet").is_err());
        assert!(helius_page(json!({"success":false,"statusCode":403}), None, "wallet").is_err());
        assert!(helius_page(
            json!({"data":[],"paginationToken":"same"}),
            Some("same"),
            "wallet"
        )
        .is_err());
        assert!(
            helius_page(json!({"data":[],"paginationToken":null}), None, "wallet")
                .unwrap()
                .exhausted
        );
    }
    #[test]
    fn inherited_and_reverted_call_values_are_never_double_counted() {
        let trace = json!({"type":"CALL","from":"wallet","to":"router","value":"0xde0b6b3a7640000","calls":[{"type":"DELEGATECALL","from":"wallet","to":"implementation","value":"0xde0b6b3a7640000"},{"type":"CALL","from":"router","to":"wallet","value":"0xde0b6b3a7640000","error":"reverted","calls":[{"type":"CALL","from":"router","to":"wallet","value":"0xde0b6b3a7640000"}]}]});
        assert_eq!(native_calls(&trace, "wallet").unwrap(), -Decimal::ONE);
    }
    #[test]
    fn solana_owned_account_rent_and_wrapping_cancel_and_fee_payer_is_exact() {
        let raw = json!({"slot":1,"transactionIndex":0,"blockTime":10,"transaction":{"signatures":["test"],"message":{"accountKeys":["wallet","ata"]}},"meta":{"err":null,"fee":5000,"preBalances":[1000000000,0],"postBalances":[997995000,2000000],"preTokenBalances":[],"postTokenBalances":[{"accountIndex":1,"mint":"token","owner":"wallet","uiTokenAmount":{"amount":"10","decimals":0}}],"logMessages":[]}});
        let tx = parse_solana(&raw, "wallet", true).unwrap();
        assert_eq!(tx.fee_quantity, Some(Decimal::new(5, 6)));
        assert!(tx.assets.iter().all(|d| d.asset != "SOL"));
        assert!(!tx.swap_evidence);
    }
    #[test]
    fn each_history_route_must_bridge_the_poll_gap() {
        let known = BTreeSet::from(["old".into()]);
        let mut page = Page {
            scan_range: None,
            records: vec![],
            cursor: Some("next".into()),
            exhausted: false,
            indexed: true,
            notes: vec![],
            segments: vec![
                (vec!["new".into(), "old".into()], false),
                (vec!["unknown".into()], false),
            ],
        };
        assert!(!head_covered(&page, &known));
        page.segments[1].1 = true;
        assert!(head_covered(&page, &known));
    }
    #[test]
    fn program_invocation_and_spoofed_instruction_are_not_a_swap() {
        let program = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
        assert!(!solana_swap_logs(&json!([
            format!("Program {program} invoke [1]"),
            "Program log: Instruction: Create"
        ])));
        assert!(!solana_swap_logs(&json!([
            format!("Program {program} invoke [1]"),
            format!("Program {TOKEN_PROGRAM} invoke [2]"),
            "Program log: spoof success",
            "Program log: Instruction: Buy"
        ])));
        assert!(solana_swap_logs(&json!([
            format!("Program {program} invoke [1]"),
            "Program log: Instruction: Buy"
        ])));
    }

    #[test]
    fn archived_rh_native_swap_matches_actual_movement_and_bundled_fee() {
        let v: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/tracker-rh-native-swap.json"
        ))
        .unwrap();
        let decimals: BTreeMap<String, u32> =
            serde_json::from_value(v["decimals"].clone()).unwrap();
        let tx = parse_evm(
            Chain::Robinhood,
            v["id"].as_str().unwrap(),
            v["wallet"].as_str().unwrap(),
            &v["transaction"],
            &v["receipt"],
            &v["trace"],
            &v["block"],
            &decimals,
            u64::MAX,
        )
        .unwrap();
        assert!(tx.movement_complete);
        assert!(tx.succeeded && tx.finalized);
        assert_eq!(
            tx.assets
                .iter()
                .find(|a| a.asset == "ETH")
                .unwrap()
                .quantity,
            -Decimal::new(3, 4)
        );
        assert!(tx.assets.iter().any(|a| a.quantity > Decimal::ZERO));
        // Synthetic large-value variant: native amounts must not be limited to
        // u64 wei (which would reject any payment larger than about 18 ETH).
        let mut large_tx = v["transaction"].clone();
        let mut large_trace = v["trace"].clone();
        large_tx["value"] = json!("0x2b5e3af16b1880000");
        large_trace["value"] = large_tx["value"].clone();
        let large = parse_evm(
            Chain::Robinhood,
            v["id"].as_str().unwrap(),
            v["wallet"].as_str().unwrap(),
            &large_tx,
            &v["receipt"],
            &large_trace,
            &v["block"],
            &decimals,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(
            large
                .assets
                .iter()
                .find(|a| a.asset == "ETH")
                .unwrap()
                .quantity,
            -Decimal::from(50)
        );
        assert!(venues::verified_v3_runtime(v["runtime"].as_str().unwrap()));
        let log = v["receipt"]["logs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["topics"][0].as_str() == Some(venues::v3_topic().as_str()))
            .unwrap();
        assert!(valid_v3_swap_log(log));
        let mut trace = v["trace"].clone();
        trace["afterEVMTransfers"][0]["value"] = json!("0x0");
        assert!(
            !arbitrum_fees_match(&trace, v["wallet"].as_str().unwrap(), tx.fee_quantity).unwrap()
        );
        let mut receipt = v["receipt"].clone();
        receipt.as_object_mut().unwrap().remove("status");
        assert!(parse_evm(
            Chain::Robinhood,
            v["id"].as_str().unwrap(),
            v["wallet"].as_str().unwrap(),
            &v["transaction"],
            &receipt,
            &v["trace"],
            &v["block"],
            &decimals,
            u64::MAX
        )
        .is_err());
    }
    #[test]
    fn eip7702_wallets_are_distinct_from_protocol_bytecode() {
        assert!(standard_evm_account("0x"));
        assert!(standard_evm_account(
            "0xef01001234567890123456789012345678901234567890"
        ));
        assert!(!standard_evm_account("0x00"));
        assert!(!standard_evm_account("0x60006000"));
        assert!(!standard_evm_account("0xef0100"));
    }
}

// Helius billing checked 2026-10-05. Keep unknown indexed methods conservative.
fn helius_cost(method: &str, params: &Value) -> u64 {
    match method {
        "getTransactionsForAddress" => {
            if params
                .pointer("/1/transactionDetails")
                .and_then(Value::as_str)
                == Some("signatures")
            {
                10
            } else {
                params
                    .pointer("/1/limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(100)
                    .div_ceil(100)
                    .max(1)
                    * 10
            }
        }
        "getAsset" | "getAssetBatch" => 10,
        "getAccountInfo"
        | "getSignaturesForAddress"
        | "getSlot"
        | "getTokenAccountsByOwner"
        | "getTransaction" => 1,
        _ => 100,
    }
}

fn das_metadata(value: &Value, assets: &[String], at: u64) -> Vec<crate::model::TokenQuote> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let asset = row["id"].as_str()?;
            if !assets.iter().any(|a| a == asset)
                || !matches!(
                    row["interface"].as_str(),
                    Some("FungibleAsset" | "FungibleToken")
                )
            {
                return None;
            }
            Some(crate::model::TokenQuote {
                asset: asset.into(),
                name: row
                    .pointer("/content/metadata/name")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                symbol: row
                    .pointer("/token_info/symbol")
                    .or_else(|| row.pointer("/content/metadata/symbol"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                decimals: row
                    .pointer("/token_info/decimals")
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok()),
                price_usd: None,
                observed_at: at,
                source: "Helius DAS fungible-token metadata".into(),
                detail: "Token identity received; a current USD market mark is unavailable.".into(),
            })
        })
        .collect()
}

#[cfg(test)]
mod credit_metadata_tests {
    use super::*;
    #[test]
    fn billing_is_weighted_and_unknown_methods_are_conservative() {
        assert_eq!(helius_cost("getTransaction", &json!([])), 1);
        assert_eq!(
            helius_cost("getTransactionsForAddress", &json!(["test",{"limit":100}])),
            10
        );
        assert_eq!(
            helius_cost("getTransactionsForAddress", &json!(["test",{"limit":1000}])),
            100
        );
        assert_eq!(helius_cost("getAssetBatch", &json!({})), 10);
        assert_eq!(helius_cost("getUnknownIndexedMethod", &json!({})), 100);
    }
    #[test]
    fn fungible_metadata_does_not_invent_identity_or_nft_market_values() {
        let value = json!([
            {"id":"test-token","interface":"FungibleToken","content":{"metadata":{"name":"Synthetic","symbol":"TEST"}},"token_info":{"decimals":9,"price_info":{"currency":"USD","price_per_token":999}}},
            {"id":"nft","interface":"V1_NFT","content":{"metadata":{"name":"NFT"}}},
            {"id":"other-chain-asset","interface":"FungibleToken"}
        ]);
        let result = das_metadata(&value, &["test-token".into(), "nft".into()], 123);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name.as_deref(), Some("Synthetic"));
        assert_eq!(result[0].decimals, Some(9));
        assert_eq!(result[0].price_usd, None);
    }
}
