use super::{HolderCandidate, HistoryCoverage, RawAssetFlow, RawHistory, RawWalletTransaction};
use futures::{stream, StreamExt};
use num_bigint::BigUint;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Mutex},
};

const TRANSFER_TOPIC: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const MAX_CANDIDATE_TRANSACTIONS: usize = 250;
const MAX_LOG_QUERIES: usize = 512;
const MAX_TRANSFER_LOGS: usize = 100_000;
const CONCURRENT_TX_FETCHES: usize = 8;
const CONCURRENT_CODE_FETCHES: usize = 3;
const MAX_HOLDER_CLASSIFICATION_CANDIDATES: usize = 256;
const MAX_HOLDER_LOG_QUERIES: usize = 2_048;
const INITIAL_HOLDER_BLOCK_WINDOW: u64 = 20_000;
const MAX_HOLDER_BLOCK_WINDOW: u64 = 80_000;
const MIN_HOLDER_BLOCK_WINDOW: u64 = 250;

#[derive(Clone)]
pub struct RobinhoodHistoryClient {
    http: reqwest::Client,
    rpc_url: String,
    holder_index_url: Option<String>,
    holder_index_key: Option<String>,
    decimals_cache: Arc<Mutex<HashMap<String, u32>>>,
    block_time_cache: Arc<Mutex<HashMap<String, u64>>>,
}

#[derive(Clone, Debug)]
pub struct RobinhoodWalletHolderSet {
    pub holders: Vec<HolderCandidate>,
    pub excluded_contracts: usize,
    pub excluded_contract_quantity: Decimal,
    pub complete_for_requested: bool,
}

impl RobinhoodHistoryClient {
    pub fn new(http: reqwest::Client, rpc_url: String) -> Self {
        Self {
            http,
            rpc_url,
            holder_index_url: None,
            holder_index_key: None,
            decimals_cache: Arc::new(Mutex::new(HashMap::new())),
            block_time_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_holder_index(
        http: reqwest::Client,
        rpc_url: String,
        holder_index_url: String,
        holder_index_key: String,
    ) -> Self {
        Self {
            http,
            rpc_url,
            holder_index_url: Some(holder_index_url.trim_end_matches('/').to_string()),
            holder_index_key: Some(holder_index_key),
            decimals_cache: Arc::new(Mutex::new(HashMap::new())),
            block_time_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn top_current_holders(
        &self,
        target_token: &str,
        limit: usize,
    ) -> Result<Vec<HolderCandidate>, String> {
        Ok(self
            .top_wallet_holders(target_token, limit)
            .await?
            .holders)
    }

    pub async fn top_wallet_holders(
        &self,
        target_token: &str,
        limit: usize,
    ) -> Result<RobinhoodWalletHolderSet, String> {
        self.indexed_wallet_holders(target_token, limit).await
    }

    pub async fn top_holder_concentration(
        &self,
        target_token: &str,
        top_n: usize,
    ) -> Result<f64, String> {
        let set = self.top_wallet_holders(target_token, top_n).await?;
        if !set.complete_for_requested {
            return Err(format!(
                "Could not prove a full top-{top_n} wallet ranking within the holder-classification safety cap."
            ));
        }

        let total = self.token_total_supply(target_token).await?;
        if total <= Decimal::ZERO {
            return Err("Robinhood ERC-20 totalSupply is zero.".to_string());
        }

        let top = set
            .holders
            .iter()
            .fold(Decimal::ZERO, |sum, holder| sum + holder.current_quantity);
        let percent = (top / total) * Decimal::from(100u32);

        percent
            .to_string()
            .parse::<f64>()
            .map(|value| value.clamp(0.0, 100.0))
            .map_err(|error| error.to_string())
    }

    pub async fn token_total_supply(
        &self,
        target_token: &str,
    ) -> Result<Decimal, String> {
        let decimals = self.token_decimals(target_token).await?;
        let value = self
            .rpc(
                "eth_call",
                json!([
                    {"to": target_token, "data": "0x18160ddd"},
                    "latest"
                ]),
            )
            .await?;
        let raw = value
            .as_str()
            .and_then(hex_biguint)
            .ok_or_else(|| "totalSupply returned an invalid uint256".to_string())?;

        biguint_to_decimal(&raw, decimals)
            .ok_or_else(|| "totalSupply exceeded Water's decimal range".to_string())
    }

    async fn indexed_wallet_holders(
        &self,
        target_token: &str,
        limit: usize,
    ) -> Result<RobinhoodWalletHolderSet, String> {
        let base = self
            .holder_index_url
            .as_ref()
            .ok_or_else(|| {
                "Robinhood wallet-holder reconstruction needs the indexed holder source; BLOCKSCOUT_API_KEY is not configured."
                    .to_string()
            })?;
        let api_key = self
            .holder_index_key
            .as_ref()
            .ok_or_else(|| "BLOCKSCOUT_API_KEY is not configured.".to_string())?;
        let decimals = self.token_decimals(target_token).await?;

        let mut url = format!("{base}/tokens/{target_token}/holders");
        let mut pages = 0usize;
        let mut holders = Vec::new();
        let mut excluded_contracts = 0usize;
        let mut excluded_contract_quantity = Decimal::ZERO;
        let mut exhausted_index = false;

        while holders.len() < limit && pages < 5 {
            let response = self
                .http
                .get(&url)
                .header("Accept", "application/json")
                .header("User-Agent", "water/0.1")
                .query(&[("apikey", api_key)])
                .send()
                .await
                .map_err(|error| format!("Blockscout holder index request failed: {error}"))?;

            let status = response.status();
            let body = response
                .json::<Value>()
                .await
                .map_err(|error| {
                    format!("Blockscout holder index returned unreadable JSON: {error}")
                })?;

            if !status.is_success() {
                return Err(format!(
                    "Blockscout holder index returned HTTP {}.",
                    status.as_u16()
                ));
            }

            let payload = body.get("data").unwrap_or(&body);
            let items = payload
                .get("items")
                .and_then(Value::as_array)
                .or_else(|| body.get("items").and_then(Value::as_array))
                .ok_or_else(|| {
                    "Blockscout holder index did not return an items array.".to_string()
                })?;

            for item in items {
                let Some(address) = indexed_holder_address(item) else {
                    continue;
                };
                if is_zero_address(&address)
                    || address.eq_ignore_ascii_case(
                        "0x000000000000000000000000000000000000dead",
                    )
                {
                    continue;
                }

                let Some(raw) = indexed_holder_raw_value(item) else {
                    continue;
                };
                let Some(quantity) = scaled_decimal(&raw, decimals) else {
                    continue;
                };
                if quantity <= Decimal::ZERO {
                    continue;
                }

                let is_contract = indexed_holder_is_contract(item).ok_or_else(|| {
                    format!(
                        "Blockscout holder index omitted contract classification for {address}."
                    )
                })?;

                if is_contract {
                    excluded_contracts += 1;
                    excluded_contract_quantity += quantity;
                    continue;
                }

                holders.push(HolderCandidate {
                    wallet: address,
                    current_quantity: quantity,
                    source_rank: holders.len() + 1,
                });

                if holders.len() >= limit {
                    break;
                }
            }

            pages += 1;

            if holders.len() >= limit {
                break;
            }

            let next = payload
                .get("next_page_params")
                .or_else(|| body.get("next_page_params"));

            let Some(next) = next.filter(|value| !value.is_null()) else {
                exhausted_index = true;
                break;
            };
            let Some(params) = next.as_object() else {
                exhausted_index = true;
                break;
            };
            if params.is_empty() {
                exhausted_index = true;
                break;
            }

            let mut next_url = reqwest::Url::parse(&format!(
                "{base}/tokens/{target_token}/holders"
            ))
            .map_err(|error| format!("Could not build Blockscout pagination URL: {error}"))?;
            {
                let mut pairs = next_url.query_pairs_mut();
                for (key, value) in params {
                    if let Some(value) = value_as_query_string(value) {
                        pairs.append_pair(key, &value);
                    }
                }
            }
            url = next_url.to_string();
        }

        if holders.is_empty() && excluded_contracts == 0 {
            return Err(
                "Blockscout holder index returned no usable ERC-20 holder balances.".to_string(),
            );
        }

        Ok(RobinhoodWalletHolderSet {
            complete_for_requested: holders.len() >= limit || exhausted_index,
            holders,
            excluded_contracts,
            excluded_contract_quantity,
        })
    }

    async fn current_holder_balances(
        &self,
        target_token: &str,
    ) -> Result<Vec<(String, Decimal)>, String> {
        let decimals = self.token_decimals(target_token).await?;
        let head = self
            .rpc("eth_blockNumber", json!([]))
            .await?
            .as_str()
            .and_then(hex_u64)
            .ok_or_else(|| "Robinhood eth_blockNumber returned invalid data.".to_string())?;

        // Best-effort deployment discovery avoids scanning millions of empty
        // pre-deployment blocks. If historical state is unavailable, the
        // adaptive pager safely falls back to block zero.
        let start_block = self
            .find_contract_deployment_block(target_token, head)
            .await
            .unwrap_or(0);

        let mut balances: HashMap<String, Decimal> = HashMap::new();
        let mut from = start_block;
        let mut window = INITIAL_HOLDER_BLOCK_WINDOW;
        let mut queries = 0usize;

        while from <= head {
            if queries >= MAX_HOLDER_LOG_QUERIES {
                return Err(format!(
                    "Robinhood wallet-holder replay hit the {MAX_HOLDER_LOG_QUERIES}-query safety cap before reaching the chain head."
                ));
            }

            let to = from
                .saturating_add(window.saturating_sub(1))
                .min(head);

            let filter = json!({
                "fromBlock": format!("0x{from:x}"),
                "toBlock": format!("0x{to:x}"),
                "address": target_token,
                "topics": [TRANSFER_TOPIC],
            });

            queries += 1;

            match self.rpc("eth_getLogs", json!([filter])).await {
                Ok(value) => {
                    let rows = value
                        .as_array()
                        .cloned()
                        .ok_or_else(|| {
                            "Robinhood eth_getLogs returned an unreadable result.".to_string()
                        })?;
                    let row_count = rows.len();

                    for log in rows {
                        let Some((sender, recipient, raw)) = parse_transfer_log(&log) else {
                            continue;
                        };
                        let Some(quantity) = biguint_to_decimal(&raw, decimals) else {
                            continue;
                        };

                        if !is_zero_address(&sender) {
                            *balances.entry(sender).or_insert(Decimal::ZERO) -= quantity;
                        }
                        if !is_zero_address(&recipient) {
                            *balances.entry(recipient).or_insert(Decimal::ZERO) += quantity;
                        }
                    }

                    if to == head {
                        break;
                    }

                    from = to.saturating_add(1);

                    // Grow aggressively through quiet history, shrink after dense
                    // windows. This adapts to each RPC's range/result ceiling.
                    if row_count < 200 {
                        window = window
                            .saturating_mul(2)
                            .min(MAX_HOLDER_BLOCK_WINDOW);
                    } else if row_count > 5_000 {
                        window = (window / 2).max(MIN_HOLDER_BLOCK_WINDOW);
                    }
                }
                Err(error) if window > MIN_HOLDER_BLOCK_WINDOW => {
                    window = (window / 2).max(MIN_HOLDER_BLOCK_WINDOW);
                    // Retry the same starting block with the smaller window.
                    if queries % 32 == 0 {
                        sleep(Duration::from_millis(250)).await;
                    }
                    let _ = error;
                }
                Err(error) => {
                    return Err(format!(
                        "Robinhood holder replay could not read blocks {from}-{to} even at the minimum adaptive window: {error}"
                    ));
                }
            }
        }

        Ok(balances
            .into_iter()
            .filter(|(wallet, quantity)| {
                *quantity > Decimal::ZERO
                    && !wallet.eq_ignore_ascii_case(
                        "0x000000000000000000000000000000000000dead",
                    )
            })
            .collect())
    }

    async fn find_contract_deployment_block(
        &self,
        address: &str,
        head: u64,
    ) -> Option<u64> {
        let latest_code = self
            .rpc(
                "eth_getCode",
                json!([address, format!("0x{head:x}")]),
            )
            .await
            .ok()?;

        if !has_contract_code(&latest_code) {
            return None;
        }

        let genesis_code = self
            .rpc("eth_getCode", json!([address, "0x0"]))
            .await
            .ok()?;

        if has_contract_code(&genesis_code) {
            return Some(0);
        }

        let mut low = 0u64;
        let mut high = head;

        while low < high {
            let mid = low + (high - low) / 2;
            let value = self
                .rpc(
                    "eth_getCode",
                    json!([address, format!("0x{mid:x}")]),
                )
                .await
                .ok()?;

            if has_contract_code(&value) {
                high = mid;
            } else {
                low = mid.saturating_add(1);
            }
        }

        Some(low)
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
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params
        });

        let mut last_error = None;

        for attempt in 0..4 {
            match self.http.post(&self.rpc_url).json(&payload).send().await {
                Ok(response) => {
                    let status = response.status();
                    let retry_after = response
                        .headers()
                        .get("retry-after")
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok())
                        .unwrap_or(1 + attempt as u64);
                    let body = response
                        .json::<Value>()
                        .await
                        .map_err(|error| error.to_string())?;

                    if status.as_u16() == 429 || status.is_server_error() {
                        last_error = Some(format!("HTTP {}: {body}", status.as_u16()));
                        sleep(Duration::from_secs(retry_after.min(5))).await;
                        continue;
                    }
                    if !status.is_success() {
                        return Err(format!("HTTP {}: {body}", status.as_u16()));
                    }
                    if let Some(error) = body.get("error") {
                        let code = error.get("code").and_then(Value::as_i64);
                        if code == Some(429) {
                            last_error = Some(error.to_string());
                            sleep(Duration::from_secs((1 + attempt as u64).min(5))).await;
                            continue;
                        }
                        return Err(error.to_string());
                    }

                    return body
                        .get("result")
                        .cloned()
                        .ok_or_else(|| "RPC response did not contain a result.".to_string());
                }
                Err(error) => {
                    last_error = Some(error.to_string());
                    sleep(Duration::from_millis(400 * (attempt + 1) as u64)).await;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| "Robinhood RPC request failed.".to_string()))
    }
}

#[derive(Clone, Debug)]
pub struct WalletHolderConcentration {
    pub percentage: f64,
    pub wallets_used: usize,
    pub excluded_contract_count: usize,
    pub excluded_contract_percentage: f64,
}

#[derive(Debug)]
struct RankedWalletHolders {
    wallets: Vec<(String, Decimal)>,
    total_balance: Decimal,
    excluded_contract_count: usize,
    excluded_contract_balance: Decimal,
}

fn decimal_percent(numerator: Decimal, denominator: Decimal) -> Result<f64, String> {
    if denominator <= Decimal::ZERO {
        return Err("Cannot calculate holder concentration with zero denominator.".to_string());
    }

    ((numerator / denominator) * Decimal::from(100u32))
        .to_string()
        .parse::<f64>()
        .map(|value| value.clamp(0.0, 100.0))
        .map_err(|error| error.to_string())
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


fn indexed_holder_address(item: &Value) -> Option<String> {
    item.pointer("/address/hash")
        .and_then(Value::as_str)
        .or_else(|| item.get("address_hash").and_then(Value::as_str))
        .or_else(|| item.get("address").and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

fn indexed_holder_raw_value(item: &Value) -> Option<String> {
    item.get("value")
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
        .or_else(|| {
            item.pointer("/balance/value")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn indexed_holder_is_contract(item: &Value) -> Option<bool> {
    item.pointer("/address/is_contract")
        .and_then(Value::as_bool)
        .or_else(|| item.get("is_contract").and_then(Value::as_bool))
}

fn value_as_query_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn has_contract_code(value: &Value) -> bool {
    value
        .as_str()
        .map(|code| code != "0x" && code != "0x0")
        .unwrap_or(false)
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
    fn parses_blockscout_holder_shapes() {
        let item = json!({
            "address": {
                "hash": "0x1111111111111111111111111111111111111111",
                "is_contract": false
            },
            "value": "2500000"
        });

        assert_eq!(
            indexed_holder_address(&item).as_deref(),
            Some("0x1111111111111111111111111111111111111111")
        );
        assert_eq!(indexed_holder_raw_value(&item).as_deref(), Some("2500000"));
    }

    #[test]
    fn parses_blockscout_contract_classification() {
        let contract = json!({
            "address": {
                "hash": "0x1111111111111111111111111111111111111111",
                "is_contract": true
            },
            "value": "1000"
        });
        let wallet = json!({
            "address": {
                "hash": "0x2222222222222222222222222222222222222222",
                "is_contract": false
            },
            "value": "900"
        });

        assert_eq!(indexed_holder_is_contract(&contract), Some(true));
        assert_eq!(indexed_holder_is_contract(&wallet), Some(false));
    }

    #[test]
    fn contract_code_detection_is_strict() {
        assert!(!has_contract_code(&json!("0x")));
        assert!(!has_contract_code(&json!("0x0")));
        assert!(has_contract_code(&json!("0x6001600055")));
    }

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
