use super::{HolderCandidate, HistoryCoverage, RawAssetFlow, RawHistory, RawWalletTransaction};
use futures::{stream, StreamExt};
use num_bigint::BigUint;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
    sync::{Arc, Mutex},
};

const TRANSFER_TOPIC: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const MAX_CANDIDATE_TRANSACTIONS: usize = 250;
const MAX_LOG_QUERIES: usize = 512;
const MAX_TRANSFER_LOGS: usize = 100_000;
const CONCURRENT_TX_FETCHES: usize = 8;

#[derive(Clone)]
pub struct RobinhoodHistoryClient {
    http: reqwest::Client,
    rpc_url: String,
    decimals_cache: Arc<Mutex<HashMap<String, u32>>>,
    block_time_cache: Arc<Mutex<HashMap<String, u64>>>,
}

impl RobinhoodHistoryClient {
    pub fn new(http: reqwest::Client, rpc_url: String) -> Self {
        Self {
            http,
            rpc_url,
            decimals_cache: Arc::new(Mutex::new(HashMap::new())),
            block_time_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn top_current_holders(
        &self,
        target_token: &str,
        limit: usize,
    ) -> Result<Vec<HolderCandidate>, String> {
        let decimals = self.token_decimals(target_token).await?;
        let scan = self
            .scan_logs(target_token, vec![Value::String(TRANSFER_TOPIC.to_string())])
            .await;

        if scan.logs.is_empty() && !scan.complete {
            return Err(scan
                .notes
                .first()
                .cloned()
                .unwrap_or_else(|| "Robinhood transfer-log scan failed.".to_string()));
        }

        let mut balances: HashMap<String, Decimal> = HashMap::new();

        for log in &scan.logs {
            let Some((from, to, raw)) = parse_transfer_log(log) else {
                continue;
            };
            let Some(quantity) = biguint_to_decimal(&raw, decimals) else {
                continue;
            };

            if !is_zero_address(&from) {
                *balances.entry(from).or_insert(Decimal::ZERO) -= quantity;
            }
            if !is_zero_address(&to) {
                *balances.entry(to).or_insert(Decimal::ZERO) += quantity;
            }
        }

        let mut rows: Vec<(String, Decimal)> = balances
            .into_iter()
            .filter(|(wallet, quantity)| {
                *quantity > Decimal::ZERO
                    && !wallet.eq_ignore_ascii_case("0x000000000000000000000000000000000000dead")
            })
            .collect();
        rows.sort_by(|left, right| right.1.cmp(&left.1));

        Ok(rows
            .into_iter()
            .take(limit)
            .enumerate()
            .map(|(index, (wallet, current_quantity))| HolderCandidate {
                wallet,
                current_quantity,
                source_rank: index + 1,
            })
            .collect())
    }

    pub async fn wallet_token_history(
        &self,
        wallet: &str,
        target_token: &str,
    ) -> RawHistory {
        let wallet_topic = address_topic(wallet);

        let outgoing = self
            .scan_logs(
                target_token,
                vec![
                    Value::String(TRANSFER_TOPIC.to_string()),
                    Value::String(wallet_topic.clone()),
                ],
            )
            .await;
        let incoming = self
            .scan_logs(
                target_token,
                vec![
                    Value::String(TRANSFER_TOPIC.to_string()),
                    Value::Null,
                    Value::String(wallet_topic),
                ],
            )
            .await;

        let mut notes = outgoing.notes;
        notes.extend(incoming.notes);

        let mut by_hash: HashMap<String, (String, u64, u64)> = HashMap::new();

        for log in outgoing.logs.into_iter().chain(incoming.logs) {
            let Some(hash) = log.get("transactionHash").and_then(Value::as_str) else {
                continue;
            };
            let block = log
                .get("blockNumber")
                .and_then(Value::as_str)
                .and_then(hex_u64)
                .unwrap_or(u64::MAX);
            let index = log
                .get("logIndex")
                .and_then(Value::as_str)
                .and_then(hex_u64)
                .unwrap_or(u64::MAX);

            by_hash
                .entry(hash.to_ascii_lowercase())
                .or_insert_with(|| (hash.to_string(), block, index));
        }

        let candidate_transactions = by_hash.len();
        let mut hashes: Vec<(String, u64, u64)> = by_hash.into_values().collect();
        hashes.sort_by_key(|(_, block, index)| (*block, *index));

        let truncated_by_tx_cap = hashes.len() > MAX_CANDIDATE_TRANSACTIONS;
        if truncated_by_tx_cap {
            notes.push(format!(
                "More than {MAX_CANDIDATE_TRANSACTIONS} target-token transactions were found; Water capped this request."
            ));
        }
        hashes.truncate(MAX_CANDIDATE_TRANSACTIONS);

        let this = self.clone();
        let wallet_owned = wallet.to_string();
        let results: Vec<Result<(RawWalletTransaction, Vec<String>), String>> =
            stream::iter(hashes.into_iter().map(move |(hash, _, _)| {
                let client = this.clone();
                let wallet = wallet_owned.clone();
                async move { client.reconstruct_transaction(&wallet, &hash).await }
            }))
            .buffer_unordered(CONCURRENT_TX_FETCHES)
            .collect()
            .await;

        let mut transactions = Vec::new();
        for result in results {
            match result {
                Ok((tx, tx_notes)) => {
                    transactions.push(tx);
                    notes.extend(tx_notes);
                }
                Err(error) => notes.push(error),
            }
        }
        transactions.sort_by_key(|tx| tx.timestamp);

        let log_complete = outgoing.complete && incoming.complete;
        let current_balance = self.current_token_balance(wallet, target_token).await.ok();
        let reconstructed_transactions = transactions.len();
        let truncated = !log_complete || truncated_by_tx_cap;

        RawHistory {
            transactions,
            coverage: HistoryCoverage {
                source: "Robinhood JSON-RPC ERC-20 Transfer logs + transaction receipts".to_string(),
                complete: log_complete && !truncated_by_tx_cap && reconstructed_transactions == candidate_transactions,
                pages_read: outgoing.queries + incoming.queries,
                candidate_transactions,
                reconstructed_transactions,
                observed_current_quantity: current_balance,
                truncated,
                notes,
            },
        }
    }

    async fn reconstruct_transaction(
        &self,
        wallet: &str,
        hash: &str,
    ) -> Result<(RawWalletTransaction, Vec<String>), String> {
        let (receipt, tx) = tokio::join!(
            self.rpc("eth_getTransactionReceipt", json!([hash])),
            self.rpc("eth_getTransactionByHash", json!([hash]))
        );
        let receipt = receipt.map_err(|error| format!("Could not fetch receipt {hash}: {error}"))?;
        let tx = tx.map_err(|error| format!("Could not fetch transaction {hash}: {error}"))?;

        let block_number = receipt
            .get("blockNumber")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("Receipt {hash} had no block number"))?;
        let timestamp = self.block_timestamp(block_number).await?;

        let mut notes = Vec::new();
        let mut raw_by_token: HashMap<String, (BigUint, BigUint)> = HashMap::new();

        for log in receipt
            .get("logs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let Some((from, to, raw)) = parse_transfer_log(&log) else {
                continue;
            };
            if !from.eq_ignore_ascii_case(wallet) && !to.eq_ignore_ascii_case(wallet) {
                continue;
            }

            let Some(token) = log.get("address").and_then(Value::as_str) else {
                continue;
            };
            let entry = raw_by_token
                .entry(token.to_ascii_lowercase())
                .or_insert_with(|| (BigUint::from(0u8), BigUint::from(0u8)));

            if from.eq_ignore_ascii_case(wallet) {
                entry.0 += &raw;
            }
            if to.eq_ignore_ascii_case(wallet) {
                entry.1 += &raw;
            }
        }

        let mut assets = Vec::new();
        for (token, (sent, received)) in raw_by_token {
            let decimals = match self.token_decimals(&token).await {
                Ok(decimals) => decimals,
                Err(error) => {
                    notes.push(format!(
                        "Could not normalize ERC-20 {token} in {hash}: {error}"
                    ));
                    continue;
                }
            };

            let sent = biguint_to_decimal(&sent, decimals).unwrap_or(Decimal::ZERO);
            let received = biguint_to_decimal(&received, decimals).unwrap_or(Decimal::ZERO);
            let delta = received - sent;
            if delta != Decimal::ZERO {
                assets.push(RawAssetFlow {
                    asset_id: token,
                    delta,
                });
            }
        }

        let from = tx.get("from").and_then(Value::as_str);
        let to = tx.get("to").and_then(Value::as_str);
        let value = tx
            .get("value")
            .and_then(Value::as_str)
            .and_then(hex_biguint)
            .and_then(|raw| biguint_to_decimal(&raw, 18))
            .unwrap_or(Decimal::ZERO);

        let mut native_delta = Decimal::ZERO;
        if from.is_some_and(|address| address.eq_ignore_ascii_case(wallet)) {
            native_delta -= value;
        }
        if to.is_some_and(|address| address.eq_ignore_ascii_case(wallet)) {
            native_delta += value;
        }
        if native_delta != Decimal::ZERO {
            assets.push(RawAssetFlow {
                asset_id: "ETH".to_string(),
                delta: native_delta,
            });
        }

        let fee_quantity = if from.is_some_and(|address| address.eq_ignore_ascii_case(wallet)) {
            transaction_fee_eth(&receipt)
        } else {
            None
        };

        if assets.iter().any(|flow| flow.asset_id.starts_with("0x"))
            && native_delta == Decimal::ZERO
            && from.is_some_and(|address| address.eq_ignore_ascii_case(wallet))
        {
            notes.push(format!(
                "Robinhood tx {hash}: standard RPC cannot observe internal ETH transfers. ERC-20 deltas are exact; a native-ETH quote leg may remain unknown."
            ));
        }

        Ok((
            RawWalletTransaction {
                tx_id: hash.to_string(),
                timestamp,
                network_fee_asset_id: fee_quantity.map(|_| "ETH".to_string()),
                network_fee_quantity: fee_quantity,
                assets,
            },
            notes,
        ))
    }

    async fn current_token_balance(
        &self,
        wallet: &str,
        target_token: &str,
    ) -> Result<Decimal, String> {
        let decimals = self.token_decimals(target_token).await?;
        let data = format!("0x70a08231{:0>64}", wallet.trim_start_matches("0x"));
        let value = self
            .rpc(
                "eth_call",
                json!([
                    {"to": target_token, "data": data},
                    "latest"
                ]),
            )
            .await?;
        let raw = value
            .as_str()
            .and_then(hex_biguint)
            .ok_or_else(|| "balanceOf returned an invalid uint256".to_string())?;

        biguint_to_decimal(&raw, decimals)
            .ok_or_else(|| "balanceOf exceeded Water's decimal range".to_string())
    }

    async fn token_decimals(&self, token: &str) -> Result<u32, String> {
        let key = token.to_ascii_lowercase();
        if let Some(value) = self
            .decimals_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).copied())
        {
            return Ok(value);
        }

        let value = self
            .rpc(
                "eth_call",
                json!([
                    {"to": token, "data": "0x313ce567"},
                    "latest"
                ]),
            )
            .await?;
        let decimals = value
            .as_str()
            .and_then(hex_biguint)
            .and_then(|value| value.to_string().parse::<u32>().ok())
            .ok_or_else(|| format!("Could not read decimals for {token}"))?;

        if let Ok(mut cache) = self.decimals_cache.lock() {
            cache.insert(key, decimals);
        }

        Ok(decimals)
    }

    async fn block_timestamp(&self, block_number: &str) -> Result<u64, String> {
        if let Some(value) = self
            .block_time_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(block_number).copied())
        {
            return Ok(value);
        }

        let block = self
            .rpc("eth_getBlockByNumber", json!([block_number, false]))
            .await?;
        let timestamp = block
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(hex_u64)
            .ok_or_else(|| format!("Block {block_number} had no timestamp"))?;

        if let Ok(mut cache) = self.block_time_cache.lock() {
            cache.insert(block_number.to_string(), timestamp);
        }

        Ok(timestamp)
    }

    async fn scan_logs(&self, address: &str, topics: Vec<Value>) -> LogScan {
        let head = match self.rpc("eth_blockNumber", json!([])).await {
            Ok(value) => match value.as_str().and_then(hex_u64) {
                Some(value) => value,
                None => {
                    return LogScan::failed("Robinhood eth_blockNumber returned invalid data");
                }
            },
            Err(error) => return LogScan::failed(format!("Robinhood head lookup failed: {error}")),
        };

        let mut ranges = vec![(0u64, head)];
        let mut logs = Vec::new();
        let mut queries = 0usize;
        let mut notes = Vec::new();
        let mut complete = true;

        while let Some((from, to)) = ranges.pop() {
            if queries >= MAX_LOG_QUERIES {
                complete = false;
                notes.push(format!(
                    "Robinhood log scan hit the {MAX_LOG_QUERIES}-query safety cap."
                ));
                break;
            }

            queries += 1;
            let filter = json!({
                "fromBlock": format!("0x{from:x}"),
                "toBlock": format!("0x{to:x}"),
                "address": address,
                "topics": topics,
            });

            match self.rpc("eth_getLogs", json!([filter])).await {
                Ok(value) => {
                    let mut rows = value.as_array().cloned().unwrap_or_default();
                    logs.append(&mut rows);
                    if logs.len() > MAX_TRANSFER_LOGS {
                        complete = false;
                        notes.push(format!(
                            "Robinhood log scan exceeded the {MAX_TRANSFER_LOGS}-log safety cap."
                        ));
                        break;
                    }
                }
                Err(error) if from < to => {
                    let middle = from + (to - from) / 2;
                    ranges.push((middle + 1, to));
                    ranges.push((from, middle));
                    if queries == 1 {
                        notes.push(format!(
                            "Robinhood RPC rejected a wide eth_getLogs range; Water automatically split it ({error})."
                        ));
                    }
                }
                Err(error) => {
                    complete = false;
                    notes.push(format!(
                        "Robinhood log scan failed at block {from}: {error}"
                    ));
                }
            }
        }

        logs.sort_by_key(|log| {
            (
                log.get("blockNumber")
                    .and_then(Value::as_str)
                    .and_then(hex_u64)
                    .unwrap_or(u64::MAX),
                log.get("logIndex")
                    .and_then(Value::as_str)
                    .and_then(hex_u64)
                    .unwrap_or(u64::MAX),
            )
        });

        LogScan {
            logs,
            queries,
            complete,
            notes,
        }
    }

    async fn rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        let response = self
            .http
            .post(&self.rpc_url)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": params
            }))
            .send()
            .await
            .map_err(|error| error.to_string())?;

        let status = response.status();
        let body = response
            .json::<Value>()
            .await
            .map_err(|error| error.to_string())?;

        if !status.is_success() {
            return Err(format!("HTTP {}: {body}", status.as_u16()));
        }
        if let Some(error) = body.get("error") {
            return Err(error.to_string());
        }

        body.get("result")
            .cloned()
            .ok_or_else(|| "RPC response did not contain a result.".to_string())
    }
}

#[derive(Debug)]
struct LogScan {
    logs: Vec<Value>,
    queries: usize,
    complete: bool,
    notes: Vec<String>,
}

impl LogScan {
    fn failed(detail: impl Into<String>) -> Self {
        Self {
            logs: Vec::new(),
            queries: 0,
            complete: false,
            notes: vec![detail.into()],
        }
    }
}

fn parse_transfer_log(log: &Value) -> Option<(String, String, BigUint)> {
    let topics = log.get("topics")?.as_array()?;
    if topics.len() < 3
        || !topics.first()?.as_str()?.eq_ignore_ascii_case(TRANSFER_TOPIC)
    {
        return None;
    }

    let from = topic_address(topics.get(1)?.as_str()?)?;
    let to = topic_address(topics.get(2)?.as_str()?)?;
    let amount = log.get("data")?.as_str().and_then(hex_biguint)?;

    Some((from, to, amount))
}

fn topic_address(topic: &str) -> Option<String> {
    let clean = topic.trim_start_matches("0x");
    if clean.len() != 64 {
        return None;
    }
    Some(format!("0x{}", &clean[24..]))
}

fn address_topic(address: &str) -> String {
    format!("0x{:0>64}", address.trim_start_matches("0x").to_ascii_lowercase())
}

fn is_zero_address(address: &str) -> bool {
    address.eq_ignore_ascii_case("0x0000000000000000000000000000000000000000")
}

fn transaction_fee_eth(receipt: &Value) -> Option<Decimal> {
    let gas = receipt.get("gasUsed")?.as_str().and_then(hex_biguint)?;
    let price = receipt
        .get("effectiveGasPrice")?
        .as_str()
        .and_then(hex_biguint)?;
    biguint_to_decimal(&(gas * price), 18)
}

fn hex_biguint(value: &str) -> Option<BigUint> {
    BigUint::parse_bytes(value.trim_start_matches("0x").as_bytes(), 16)
}

fn hex_u64(value: &str) -> Option<u64> {
    u64::from_str_radix(value.trim_start_matches("0x"), 16).ok()
}

fn biguint_to_decimal(value: &BigUint, decimals: u32) -> Option<Decimal> {
    scaled_decimal(&value.to_string(), decimals)
}

fn scaled_decimal(raw: &str, decimals: u32) -> Option<Decimal> {
    if !raw.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }

    if decimals == 0 {
        return Decimal::from_str(raw).ok();
    }

    let decimals = decimals as usize;
    let normalized = if raw.len() <= decimals {
        format!("0.{}{}", "0".repeat(decimals - raw.len()), raw)
    } else {
        let split = raw.len() - decimals;
        format!("{}.{}", &raw[..split], &raw[split..])
    };

    Decimal::from_str(&normalized).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_transfer_topics_and_uint256() {
        let log = json!({
            "topics": [
                TRANSFER_TOPIC,
                "0x0000000000000000000000001111111111111111111111111111111111111111",
                "0x0000000000000000000000002222222222222222222222222222222222222222"
            ],
            "data": "0x1bc16d674ec80000"
        });

        let (from, to, amount) = parse_transfer_log(&log).unwrap();
        assert_eq!(from, "0x1111111111111111111111111111111111111111");
        assert_eq!(to, "0x2222222222222222222222222222222222222222");
        assert_eq!(biguint_to_decimal(&amount, 18), Some(Decimal::from(2)));
    }

    #[test]
    fn wallet_topic_is_left_padded() {
        assert_eq!(
            address_topic("0x1111111111111111111111111111111111111111"),
            "0x0000000000000000000000001111111111111111111111111111111111111111"
        );
    }

    #[test]
    fn uint256_scaling_handles_values_larger_than_u128() {
        let raw = BigUint::parse_bytes(
            b"1000000000000000000000000000000000000000000000",
            10,
        )
        .unwrap();

        // The human value still must fit rust_decimal; oversized results remain unknown.
        assert!(biguint_to_decimal(&raw, 18).is_some());
    }
}
