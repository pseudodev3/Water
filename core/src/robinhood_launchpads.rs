//! Exact factory membership, independent of explorer labels and token self-reports.
//! Address/ABI provenance: docs/robinhood-launchpads.md.
use crate::origin::LaunchpadEvidence;
use futures::{future::join_all, stream, StreamExt};
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
    // Independent deadline: a slow history lookup must not discard Pons/NOXA evidence.
    let (results, long) = tokio::join!(
        factories,
        timeout(
            Duration::from_millis(2500),
            detect_long(http, rpc_url, token)
        )
    );
    let matches: Vec<_> = results
        .into_iter()
        .flatten()
        .chain(long.ok().flatten())
        .collect();
    // Conflicting factory claims must not silently pick the first response.
    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}

const LONG_LAUNCHER: &str = "0x22e99278308b393ea1260859b181ad7e78f5eeed";
const LONG_TOPIC: &str = "0xadc6f1f726f7c710f77ec06adc75f3bb964e5be19581b072c67f7b9b4039267b";
const LONG_DEPLOYMENT: u64 = 8_636_038;
const LOG_WINDOW: u64 = 10_000_000;

async fn detect_long(http: &Client, url: &str, token: &str) -> Option<FactoryMatch> {
    // This is only a request-saving prefilter, never launchpad evidence. Long's
    // trusted token factory currently deploys this minimal-proxy layout.
    let code = rpc(http, url, "eth_getCode", json!([token, "latest"]))
        .await
        .ok()?;
    if !is_long_candidate(code.as_str()?) {
        return None;
    }
    let head = rpc(http, url, "eth_blockNumber", json!([])).await.ok()?;
    let head = hex_u64(head.as_str()?)?;
    let topic = format!(
        "0x{:0>64}",
        token.trim_start_matches("0x").to_ascii_lowercase()
    );
    let windows = long_windows(head);
    let mut queries = stream::iter(windows.into_iter().map(|(from, to)| {
        let topic = &topic;
        async move {
            let logs = rpc(
                http,
                url,
                "eth_getLogs",
                json!([{
                    "address": LONG_LAUNCHER, "fromBlock": format!("0x{from:x}"),
                    "toBlock": format!("0x{to:x}"), "topics": [LONG_TOPIC, null, topic]
                }]),
            )
            .await
            .ok()?;
            logs.as_array()?
                .iter()
                .find_map(|log| long_event(log, token, from, to))
        }
    }))
    .buffer_unordered(4);
    while let Some(found) = queries.next().await {
        if found.is_some() {
            return found;
        }
    }
    None
}

fn is_long_candidate(code: &str) -> bool {
    code.len() == 90
        && code.starts_with("0x3d3d3d3d363d3d37363d73")
        && code.ends_with("5af43d3d93803e602a57fd5bf3")
        && code[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn hex_u64(value: &str) -> Option<u64> {
    u64::from_str_radix(value.strip_prefix("0x")?, 16).ok()
}

fn long_windows(head: u64) -> Vec<(u64, u64)> {
    if head < LONG_DEPLOYMENT {
        return vec![];
    }
    let count = (head - LONG_DEPLOYMENT) / LOG_WINDOW + 1;
    // At most 24 windows, interleaved oldest/newest, four in flight, 2.5s total.
    // An incomplete search is unknown, never evidence of a different launchpad.
    (0..count.min(24))
        .map(|i| {
            let index = if i % 2 == 0 { i / 2 } else { count - 1 - i / 2 };
            let from = LONG_DEPLOYMENT + index * LOG_WINDOW;
            (from, head.min(from.saturating_add(LOG_WINDOW - 1)))
        })
        .collect()
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

fn long_event(log: &Value, token: &str, from: u64, to: u64) -> Option<FactoryMatch> {
    if !log
        .get("address")?
        .as_str()?
        .eq_ignore_ascii_case(LONG_LAUNCHER)
        || log.get("removed")?.as_bool()?
    {
        return None;
    }
    let block = hex_u64(log.get("blockNumber")?.as_str()?)?;
    if !(from..=to).contains(&block) {
        return None;
    }
    let topics = log.get("topics")?.as_array()?;
    if topics.len() != 4 || topics[0].as_str()? != LONG_TOPIC {
        return None;
    }
    // asset is indexed topic 2; topic 1 is poolOrHook and topic 3 is the quote asset.
    if !address_word(topics[2].as_str()?.strip_prefix("0x")?)?.eq_ignore_ascii_case(token) {
        return None;
    }
    address_word(topics[1].as_str()?.strip_prefix("0x")?)?;
    address_word(topics[3].as_str()?.strip_prefix("0x")?)?;
    let data = log.get("data")?.as_str()?.strip_prefix("0x")?;
    // Six head words, string length, one padded 1..15-byte normalized ticker.
    if data.len() != 512 || !data.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    address_word(&data[..64])?;
    let creator = address_word(&data[64..128])?;
    let deployed = u64::from_str_radix(&data[192..256], 16).ok()?;
    let reserved = u64::from_str_radix(&data[256..320], 16).ok()?;
    if deployed == 0
        || reserved >= (1 << 48)
        || reserved.checked_sub(deployed)? != 86400
        || u64::from_str_radix(&data[320..384], 16).ok()? != 192
    {
        return None;
    }
    let length = usize::from_str_radix(&data[384..448], 16).ok()?;
    if !(1..=15).contains(&length) {
        return None;
    }
    for i in 0..32 {
        let byte = u8::from_str_radix(&data[448 + i * 2..450 + i * 2], 16).ok()?;
        if (i < length && !byte.is_ascii_uppercase()) || (i >= length && byte != 0) {
            return None;
        }
    }
    Some(FactoryMatch {
        creator,
        evidence: LaunchpadEvidence {
            slug: "longxyz".to_string(), name: "Long.xyz".to_string(),
            family: "Robinhood Chain launchpad".to_string(),
            source: "Robinhood verified launcher event".to_string(),
            evidence: format!("LongLauncher {LONG_LAUNCHER} emitted LaunchCreated for this exact token at block {block}. This verifies the onchain launcher, not which website submitted the launch."),
        },
    })
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
    const LONG_TOKEN: &str = "0x2E8c31162b855A2ffa90F6F8634643Ad6F111e18";
    const CLONE: &str = "0x3d3d3d3d363d3d37363d733be8b97fd0e713b5abe0649fa830223b6b4bc5995af43d3d93803e602a57fd5bf3";
    fn launch() -> Value {
        serde_json::from_str(include_str!("../fixtures/long-launch-created.json")).unwrap()
    }
    #[test]
    fn long_real_event_identifies_exact_asset_and_launch_caller() {
        let found = long_event(&launch(), LONG_TOKEN, LONG_DEPLOYMENT, 18_636_037).unwrap();
        assert_eq!(found.evidence.slug, "longxyz");
        assert_eq!(found.creator, "0x9b1513dfdfc023fa6e576b130066ec05b6f1bfa1");
        assert!(is_long_candidate(CLONE));
        assert!(!is_long_candidate("0x"));
        assert!(long_event(
            &launch(),
            "0xd0601ce157db5bdc3162bbac2a2c8af5320d9eec",
            0,
            u64::MAX
        )
        .is_none());
    }
    #[test]
    fn long_rejects_spoofed_removed_truncated_and_wrong_position_logs() {
        for field in ["address", "topics", "data", "removed", "blockNumber"] {
            let mut log = launch();
            log.as_object_mut().unwrap().remove(field);
            assert!(long_event(&log, LONG_TOKEN, 0, u64::MAX).is_none());
        }
        let mut log = launch();
        log["address"] = json!("0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862");
        assert!(long_event(&log, LONG_TOKEN, 0, u64::MAX).is_none());
        log = launch();
        log["removed"] = json!(true);
        assert!(long_event(&log, LONG_TOKEN, 0, u64::MAX).is_none());
        log = launch();
        log["topics"][2] = log["topics"][3].clone();
        assert!(long_event(&log, LONG_TOKEN, 0, u64::MAX).is_none());
        log = launch();
        log["topics"][0] = json!("0x00");
        assert!(long_event(&log, LONG_TOKEN, 0, u64::MAX).is_none());
        log = launch();
        log["data"] = json!("0x00");
        assert!(long_event(&log, LONG_TOKEN, 0, u64::MAX).is_none());
        assert!(long_event(&launch(), LONG_TOKEN, 0, 1).is_none());
    }
    #[test]
    fn long_window_search_is_bounded_and_nonoverlapping() {
        assert!(long_windows(LONG_DEPLOYMENT - 1).is_empty());
        let mut windows = long_windows(78_022_905);
        windows.sort_unstable();
        assert_eq!(windows.len(), 7);
        assert_eq!(windows[0].0, LONG_DEPLOYMENT);
        assert_eq!(windows.last().unwrap().1, 78_022_905);
        assert!(windows.windows(2).all(|w| w[0].1 + 1 == w[1].0));
        let windows = long_windows(u64::MAX);
        assert_eq!(windows.len(), 24);
        assert!(windows.iter().all(|(a, b)| b - a < LOG_WINDOW));
    }
    #[tokio::test]
    async fn slow_long_history_preserves_existing_factory_match() {
        use axum::{routing::post, Json, Router};
        let app = Router::new().route(
            "/",
            post(|Json(body): Json<Value>| async move {
                let result = match body["method"].as_str().unwrap() {
                    "eth_chainId" => json!("0x1237"),
                    "eth_getCode" => {
                        tokio::time::sleep(Duration::from_secs(6)).await;
                        json!(CLONE)
                    }
                    "eth_call" if body["params"][0]["to"] == FACTORIES[0].address => {
                        json!(record(&FACTORIES[0], true))
                    }
                    _ => json!("0x"),
                };
                Json(json!({"jsonrpc":"2.0","id":1,"result":result}))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let start = std::time::Instant::now();
        let found = detect(&Client::new(), &url, TOKEN).await.unwrap();
        assert_eq!(found.evidence.slug, "pons");
        assert!(start.elapsed() < Duration::from_millis(3500));
        task.abort();
    }
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
}
