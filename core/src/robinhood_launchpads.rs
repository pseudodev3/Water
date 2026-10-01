//! Exact factory membership, independent of explorer labels and token self-reports.
//! Address/ABI provenance: docs/robinhood-launchpads.md.
use crate::origin::LaunchpadEvidence;
use futures::future::join_all;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::time::{timeout, Duration};

#[derive(Clone, Copy)]
struct Factory {
    address: &'static str,
    name: &'static str,
    slug: &'static str,
    version: &'static str,
    words: usize,
    exists_word: usize,
    deployer_word: usize,
}

const FACTORIES: &[Factory] = &[
    Factory { address: "0x7eD598BcEf8bd9Edd8C97A195C6d13f40801EC7e", name: "Pons", slug: "pons", version: "v2", words: 15, exists_word: 14, deployer_word: 2 },
    Factory { address: "0xA5aAb3F0c6EeadF30Ef1D3Eb997108E976351feB", name: "Pons", slug: "pons", version: "v1", words: 13, exists_word: 11, deployer_word: 1 },
    Factory { address: "0x0c37a24F5D23A486FA692d1500881d698B1F77a4", name: "Pons", slug: "pons", version: "legacy v1", words: 13, exists_word: 11, deployer_word: 1 },
    Factory { address: "0xD9eC2db5f3D1b236843925949fe5bd8a3836FCcB", name: "NOXA Fun", slug: "noxa", version: "v1", words: 13, exists_word: 11, deployer_word: 1 },
];

pub struct FactoryMatch {
    pub evidence: LaunchpadEvidence,
    pub creator: String,
}

pub async fn detect(http: &Client, rpc_url: &str, token: &str) -> Option<FactoryMatch> {
    // Never apply a chain-specific address registry to a different configured chain.
    let chain = rpc(http, rpc_url, "eth_chainId", json!([])).await.ok()?;
    if chain.as_str().and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok()) != Some(4663) {
        return None;
    }
    let calldata = format!("0x3cf28b5a{:0>64}", token.trim_start_matches("0x"));
    let results = join_all(FACTORIES.iter().copied().map(|factory| {
        let calldata = &calldata;
        async move {
            let value = rpc(http, rpc_url, "eth_call", json!([
                {"to": factory.address, "data": calldata}, "latest"
            ])).await.ok()?;
            let creator = membership_creator(value.as_str()?, token, &factory)?;
            Some(FactoryMatch {
                creator,
                evidence: LaunchpadEvidence {
                    slug: factory.slug.to_string(),
                    name: factory.name.to_string(),
                    family: "Robinhood Chain launchpad".to_string(),
                    source: "Robinhood verified factory registry".to_string(),
                    evidence: format!("{} {} factory {} reports this exact token in getLaunchedToken with exists=true.", factory.name, factory.version, factory.address),
                },
            })
        }
    })).await;
    let matches: Vec<_> = results.into_iter().flatten().collect();
    // Conflicting factory claims must not silently pick the first response.
    if matches.len() == 1 { matches.into_iter().next() } else { None }
}

fn membership_creator(encoded: &str, token: &str, factory: &Factory) -> Option<String> {
    let hex = encoded.strip_prefix("0x")?;
    if hex.len() != factory.words * 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let words: Vec<_> = hex.as_bytes().chunks_exact(64).map(|w| std::str::from_utf8(w).unwrap()).collect();
    let expected_token = format!("{:0>64}", token.trim_start_matches("0x"));
    if !words[0].eq_ignore_ascii_case(&expected_token) || words[factory.exists_word] != format!("{:0>64}", "1") {
        return None;
    }
    let creator = words[factory.deployer_word];
    if !creator.starts_with("000000000000000000000000") || creator.bytes().all(|b| b == b'0') {
        return None;
    }
    Some(format!("0x{}", &creator[24..]))
}

async fn rpc(http: &Client, url: &str, method: &str, params: Value) -> Result<Value, String> {
    timeout(Duration::from_millis(2500), async {
        let response = http.post(url).json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() { return Err(format!("RPC HTTP {}", response.status())); }
        let body: Value = response.json().await.map_err(|e| e.to_string())?;
        if let Some(error) = body.get("error") { return Err(error.to_string()); }
        body.get("result").cloned().ok_or_else(|| "Missing RPC result".to_string())
    }).await.map_err(|_| "Factory verification exceeded provider budget".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    const TOKEN: &str = "0x39dBED3a2bd333467115dE45665cC57F813C4571";
    fn record(factory: &Factory, exists: bool) -> String {
        let mut words = vec!["0".repeat(64); factory.words];
        words[0] = format!("{:0>64}", &TOKEN[2..]);
        words[factory.deployer_word] = format!("{:0>64}", "1234567890123456789012345678901234567890");
        words[factory.exists_word] = format!("{:0>64}", if exists { "1" } else { "0" });
        format!("0x{}", words.join(""))
    }
    #[test]
    fn membership_requires_exact_token_and_exists_for_both_abis() {
        for factory in FACTORIES {
            assert!(membership_creator(&record(factory, true), TOKEN, factory).is_some());
            assert!(membership_creator(&record(factory, false), TOKEN, factory).is_none());
            assert!(membership_creator(&record(factory, true), "0x1111111111111111111111111111111111111111", factory).is_none());
            assert!(membership_creator("0x", TOKEN, factory).is_none());
            assert!(membership_creator(&format!("{}00", record(factory,true)), TOKEN, factory).is_none());
        }
    }
}
