//! Exact factory membership / launch events, independent of explorer labels and token self-reports.
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
    Factory {
        address: "0x7eD598BcEf8bd9Edd8C97A195C6d13f40801EC7e",
        name: "Pons",
        slug: "pons",
        version: "v2",
        words: 15,
        exists_word: 14,
        deployer_word: 2,
    },
    Factory {
        address: "0xA5aAb3F0c6EeadF30Ef1D3Eb997108E976351feB",
        name: "Pons",
        slug: "pons",
        version: "v1",
        words: 13,
        exists_word: 11,
        deployer_word: 1,
    },
    Factory {
        address: "0x0c37a24F5D23A486FA692d1500881d698B1F77a4",
        name: "Pons",
        slug: "pons",
        version: "legacy v1",
        words: 13,
        exists_word: 11,
        deployer_word: 1,
    },
    Factory {
        address: "0xD9eC2db5f3D1b236843925949fe5bd8a3836FCcB",
        name: "NOXA Fun",
        slug: "noxa",
        version: "v1",
        words: 13,
        exists_word: 11,
        deployer_word: 1,
    },
];

pub struct FactoryMatch {
    pub evidence: LaunchpadEvidence,
    pub creator: String,
}

pub async fn detect(http: &Client, rpc_url: &str, token: &str) -> Option<FactoryMatch> {
    // Never apply a chain-specific address registry to a different configured chain.
    let chain = rpc(http, rpc_url, "eth_chainId", json!([])).await.ok()?;
    if chain
        .as_str()
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        != Some(4663)
    {
        return None;
    }
    let calldata = format!("0x3cf28b5a{:0>64}", token.trim_start_matches("0x"));
    let factories = join_all(FACTORIES.iter().copied().map(|factory| {
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
    }));
    // Keep independent factory evidence usable even when historical logs stall.
    let (results, long) = tokio::join!(factories, async {
        timeout(
            Duration::from_millis(4500),
            detect_long(http, rpc_url, token),
        )
        .await
        .ok()
        .flatten()
    });
    let matches: Vec<_> = results.into_iter().flatten().chain(long).collect();
    // Conflicting factory claims must not silently pick the first response.
    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}

// Exact-match deployed source includes "Copyright (c) 2026 long.xyz".
const LONG_LAUNCHER: &str = "0x22e99278308b393ea1260859b181ad7e78f5eeed";
const LONG_DEPLOYMENT_BLOCK: u64 = 8_636_038;
const LONG_LAUNCH_TOPIC: &str =
    "0xadc6f1f726f7c710f77ec06adc75f3bb964e5be19581b072c67f7b9b4039267b";
const LOG_RANGE: u64 = 10_000_000;
const MAX_LOG_RANGES: usize = 16;

async fn detect_long(http: &Client, url: &str, token: &str) -> Option<FactoryMatch> {
    let latest = rpc(http, url, "eth_blockNumber", json!([])).await.ok()?;
    let latest = hex_u64(latest.as_str()?)?;
    let ranges = long_log_ranges(latest)?;
    let token_topic = format!("0x{:0>64}", token.strip_prefix("0x")?);
    let results = join_all(ranges.into_iter().map(|(start, end)| {
        let token_topic = &token_topic;
        async move {
            rpc(
                http,
                url,
                "eth_getLogs",
                json!([{
                    "address": LONG_LAUNCHER,
                    "fromBlock": format!("0x{start:x}"),
                    "toBlock": format!("0x{end:x}"),
                    "topics": [LONG_LAUNCH_TOPIC, null, token_topic]
                }]),
            )
            .await
            .ok()
            .map(|result| (start, end, result))
        }
    }))
    .await;
    let mut found = Vec::new();
    for result in results {
        // A partial search cannot rule out conflicting launch records.
        let (start, end, result) = result?;
        for log in result.as_array()? {
            if hex_u64(log.get("blockNumber")?.as_str()?)? < start {
                return None;
            }
            let (creator, transaction) = long_event(log, token, end)?;
            found.push(FactoryMatch {
                creator,
                evidence: LaunchpadEvidence {
                    slug: "longxyz".to_string(),
                    name: "Long.xyz".to_string(),
                    family: "Robinhood Chain launchpad".to_string(),
                    source: "Robinhood verified launcher event".to_string(),
                    evidence: format!("Long.xyz LongLauncher {LONG_LAUNCHER} emitted LaunchCreated for this exact token in transaction {transaction}. Verified launcher use; frontend identity is not recorded onchain."),
                },
            });
        }
    }
    if found.len() == 1 {
        found.pop()
    } else {
        None
    }
}

fn long_log_ranges(latest: u64) -> Option<Vec<(u64, u64)>> {
    if latest < LONG_DEPLOYMENT_BLOCK {
        return None;
    }
    let count = (latest - LONG_DEPLOYMENT_BLOCK) / LOG_RANGE + 1;
    if count > MAX_LOG_RANGES as u64 {
        return None;
    }
    Some(
        (0..count)
            .map(|i| {
                let start = LONG_DEPLOYMENT_BLOCK + i * LOG_RANGE;
                (start, (start + LOG_RANGE - 1).min(latest))
            })
            .collect(),
    )
}

fn hex_u64(value: &str) -> Option<u64> {
    u64::from_str_radix(value.strip_prefix("0x")?, 16).ok()
}

fn hex_payload(value: &str, bytes: usize) -> Option<&str> {
    let value = value.strip_prefix("0x")?;
    (value.len() == bytes * 2 && value.bytes().all(|b| b.is_ascii_hexdigit())).then_some(value)
}

fn address_word(word: &str) -> Option<String> {
    if word.len() != 64
        || !word.bytes().all(|b| b.is_ascii_hexdigit())
        || !word.starts_with("000000000000000000000000")
        || word.bytes().all(|b| b == b'0')
    {
        return None;
    }
    Some(format!("0x{}", &word[24..]))
}

fn long_event(log: &Value, token: &str, latest: u64) -> Option<(String, String)> {
    if !log
        .get("address")?
        .as_str()?
        .eq_ignore_ascii_case(LONG_LAUNCHER)
        || log.get("removed")?.as_bool()?
    {
        return None;
    }
    let block = hex_u64(log.get("blockNumber")?.as_str()?)?;
    if !(LONG_DEPLOYMENT_BLOCK..=latest).contains(&block) {
        return None;
    }
    hex_payload(log.get("blockHash")?.as_str()?, 32)?;
    let transaction = log.get("transactionHash")?.as_str()?;
    hex_payload(transaction, 32)?;
    let topics = log.get("topics")?.as_array()?;
    if topics.len() != 4
        || !topics[0].as_str()?.eq_ignore_ascii_case(LONG_LAUNCH_TOPIC)
        || !address_word(hex_payload(topics[2].as_str()?, 32)?)?.eq_ignore_ascii_case(token)
    {
        return None;
    }
    address_word(hex_payload(topics[1].as_str()?, 32)?)?;
    // Native ETH is a valid zero numeraire; require canonical address padding.
    let numeraire = hex_payload(topics[3].as_str()?, 32)?;
    if !numeraire.starts_with("000000000000000000000000") {
        return None;
    }
    // Six static head words, then string length and one padded ticker word.
    let data = hex_payload(log.get("data")?.as_str()?, 8 * 32)?;
    let words: Vec<_> = data
        .as_bytes()
        .chunks_exact(64)
        .map(|w| std::str::from_utf8(w).unwrap())
        .collect();
    address_word(words[0])?;
    let creator = address_word(words[1])?;
    let deployed = u64::from_str_radix(words[3], 16).ok()?;
    let reserved = u64::from_str_radix(words[4], 16).ok()?;
    if deployed == 0
        || deployed >= (1u64 << 48)
        || reserved >= (1u64 << 48)
        || reserved != deployed + 86_400
        || u64::from_str_radix(words[5], 16).ok()? != 192
    {
        return None;
    }
    let length = usize::from_str_radix(words[6], 16).ok()?;
    if !(1..=15).contains(&length) || !words[7][length * 2..].bytes().all(|b| b == b'0') {
        return None;
    }
    for i in 0..length {
        let letter = u8::from_str_radix(&words[7][i * 2..i * 2 + 2], 16).ok()?;
        if !letter.is_ascii_uppercase() {
            return None;
        }
    }
    Some((creator, transaction.to_string()))
}

fn membership_creator(encoded: &str, token: &str, factory: &Factory) -> Option<String> {
    let hex = encoded.strip_prefix("0x")?;
    if hex.len() != factory.words * 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let words: Vec<_> = hex
        .as_bytes()
        .chunks_exact(64)
        .map(|w| std::str::from_utf8(w).unwrap())
        .collect();
    let expected_token = format!("{:0>64}", token.trim_start_matches("0x"));
    if !words[0].eq_ignore_ascii_case(&expected_token)
        || words[factory.exists_word] != format!("{:0>64}", "1")
    {
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
        let response = http
            .post(url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("RPC HTTP {}", response.status()));
        }
        let body: Value = response.json().await.map_err(|e| e.to_string())?;
        if let Some(error) = body.get("error") {
            return Err(error.to_string());
        }
        body.get("result")
            .cloned()
            .ok_or_else(|| "Missing RPC result".to_string())
    })
    .await
    .map_err(|_| "Factory verification exceeded provider budget".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    const TOKEN: &str = "0x39dBED3a2bd333467115dE45665cC57F813C4571";
    fn record(factory: &Factory, exists: bool) -> String {
        let mut words = vec!["0".repeat(64); factory.words];
        words[0] = format!("{:0>64}", &TOKEN[2..]);
        words[factory.deployer_word] =
            format!("{:0>64}", "1234567890123456789012345678901234567890");
        words[factory.exists_word] = format!("{:0>64}", if exists { "1" } else { "0" });
        format!("0x{}", words.join(""))
    }
    #[test]
    fn membership_requires_exact_token_and_exists_for_both_abis() {
        for factory in FACTORIES {
            assert!(membership_creator(&record(factory, true), TOKEN, factory).is_some());
            assert!(membership_creator(&record(factory, false), TOKEN, factory).is_none());
            assert!(membership_creator(
                &record(factory, true),
                "0x1111111111111111111111111111111111111111",
                factory
            )
            .is_none());
            assert!(membership_creator("0x", TOKEN, factory).is_none());
            assert!(
                membership_creator(&format!("{}00", record(factory, true)), TOKEN, factory)
                    .is_none()
            );
        }
    }
    fn launch_log() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/long-launch-created.json")).unwrap()
    }
    const LONG_TOKEN: &str = "0x782a6a4653896be2ff3fd885d50895d7e19a1e18";

    #[test]
    fn long_event_requires_exact_emitter_token_and_canonical_abi() {
        let log = launch_log();
        let (creator, tx) = long_event(&log, LONG_TOKEN, 78_941_234).unwrap();
        assert_eq!(creator, "0xa99c430efbdf4d2d4110d32978a2eceee21ce79a");
        assert_eq!(tx, log["transactionHash"].as_str().unwrap());
        assert!(long_event(&log, TOKEN, 78_941_234).is_none());
        for (key, value) in [
            (
                "address",
                json!("0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862"),
            ),
            ("removed", json!(true)),
            ("blockNumber", json!("0x1")),
            ("transactionHash", json!("0x1234")),
            ("data", json!("0x")),
            (
                "data",
                json!(format!("{}00", log["data"].as_str().unwrap())),
            ),
        ] {
            let mut bad = log.clone();
            bad[key] = value;
            assert!(long_event(&bad, LONG_TOKEN, 78_941_234).is_none(), "{key}");
        }
        for index in [0usize, 1, 3, 4, 5, 6, 7] {
            let mut bad = log.clone();
            let mut data = bad["data"].as_str().unwrap().to_string();
            data.replace_range(2 + index * 64..2 + (index + 1) * 64, &"f".repeat(64));
            bad["data"] = json!(data);
            assert!(long_event(&bad, LONG_TOKEN, 78_941_234).is_none());
        }
    }

    #[test]
    fn long_history_ranges_are_bounded_complete_and_nonoverlapping() {
        let latest = LONG_DEPLOYMENT_BLOCK + LOG_RANGE * 2;
        assert_eq!(
            long_log_ranges(latest).unwrap(),
            vec![
                (LONG_DEPLOYMENT_BLOCK, LONG_DEPLOYMENT_BLOCK + LOG_RANGE - 1),
                (LONG_DEPLOYMENT_BLOCK + LOG_RANGE, latest - 1),
                (latest, latest)
            ]
        );
        assert!(long_log_ranges(LONG_DEPLOYMENT_BLOCK - 1).is_none());
        assert!(
            long_log_ranges(LONG_DEPLOYMENT_BLOCK + LOG_RANGE * MAX_LOG_RANGES as u64).is_none()
        );
    }

    async fn mock_rpc(mode: &'static str) -> (Client, String, tokio::task::JoinHandle<()>) {
        use axum::{routing::post, Json, Router};
        let handler = move |Json(request): Json<Value>| async move {
            let result = match request["method"].as_str().unwrap() {
                "eth_chainId" => json!(if mode == "wrong_chain" {
                    "0x1"
                } else {
                    "0x1237"
                }),
                "eth_blockNumber" => json!("0x4b48b32"),
                "eth_call" => {
                    if mode == "factory" && request["params"][0]["to"] == FACTORIES[0].address {
                        json!(record(&FACTORIES[0], true))
                    } else {
                        json!("0x")
                    }
                }
                "eth_getLogs" => {
                    if mode == "factory" {
                        std::future::pending::<()>().await;
                    }
                    let start =
                        hex_u64(request["params"][0]["fromBlock"].as_str().unwrap()).unwrap();
                    let end = hex_u64(request["params"][0]["toBlock"].as_str().unwrap()).unwrap();
                    assert!(end - start < LOG_RANGE);
                    assert_eq!(request["params"][0]["address"], LONG_LAUNCHER);
                    assert_eq!(
                        request["params"][0]["topics"][2],
                        format!("0x{:0>64}", &LONG_TOKEN[2..])
                    );
                    if mode == "partial" && start == LONG_DEPLOYMENT_BLOCK {
                        return Json(
                            json!({"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"unavailable"}}),
                        );
                    }
                    let log = launch_log();
                    let block = hex_u64(log["blockNumber"].as_str().unwrap()).unwrap();
                    if (start..=end).contains(&block) {
                        if mode == "conflict" {
                            json!([log.clone(), log])
                        } else {
                            json!([log])
                        }
                    } else {
                        json!([])
                    }
                }
                _ => panic!("unexpected RPC method"),
            };
            Json(json!({"jsonrpc":"2.0","id":1,"result":result}))
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().route("/", post(handler));
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (Client::new(), url, task)
    }

    #[tokio::test]
    async fn long_detection_requires_complete_unambiguous_history_on_the_right_chain() {
        for mode in ["positive", "partial", "conflict", "wrong_chain"] {
            let (http, url, task) = mock_rpc(mode).await;
            let result = detect(&http, &url, LONG_TOKEN).await;
            assert_eq!(result.is_some(), mode == "positive", "{mode}");
            if let Some(result) = result {
                assert_eq!(result.evidence.slug, "longxyz");
            }
            task.abort();
        }
    }

    #[tokio::test]
    async fn stalled_long_history_preserves_existing_factory_evidence() {
        let (http, url, task) = mock_rpc("factory").await;
        let started = std::time::Instant::now();
        // Fixture factory token differs from LONG_TOKEN; log requests are not examined
        // by the stalled handler, and the independent factory match must survive.
        let result = detect(&http, &url, TOKEN).await.unwrap();
        assert_eq!(result.evidence.slug, "pons");
        assert!(started.elapsed() < Duration::from_secs(6));
        task.abort();
    }
}
