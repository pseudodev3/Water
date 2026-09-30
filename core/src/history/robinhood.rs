use super::{HistoryCoverage, RawAssetFlow, RawHistory, RawWalletTransaction};
use chrono::DateTime;
use futures::{stream, StreamExt};
use reqwest::Url;
use rust_decimal::Decimal;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

const MAX_TARGET_TRANSFER_PAGES: usize = 20;
const MAX_CANDIDATE_TRANSACTIONS: usize = 250;
const CONCURRENT_TX_FETCHES: usize = 8;
const MAX_TX_SUBRESOURCE_PAGES: usize = 4;

#[derive(Clone)]
pub struct RobinhoodHistoryClient {
    http: reqwest::Client,
    blockscout_url: String,
}

impl RobinhoodHistoryClient {
    pub fn new(http: reqwest::Client, blockscout_url: String) -> Self {
        Self {
            http,
            blockscout_url: blockscout_url.trim_end_matches('/').to_string(),
        }
    }

    pub async fn wallet_token_history(
        &self,
        wallet: &str,
        target_token: &str,
    ) -> RawHistory {
        let base = format!(
            "{}/api/v2/addresses/{wallet}/token-transfers",
            self.blockscout_url
        );

        let first_url = match Url::parse_with_params(
            &base,
            &[
                ("type", "ERC-20"),
                ("token", target_token),
            ],
        ) {
            Ok(url) => url,
            Err(error) => {
                return empty_history(format!("Could not construct Blockscout URL: {error}"));
            }
        };

        let transfer_pages = self
            .fetch_pages(first_url, MAX_TARGET_TRANSFER_PAGES)
            .await;

        let mut hashes = Vec::new();
        let mut seen = HashSet::new();

        for item in &transfer_pages.items {
            if let Some(hash) = item.get("transaction_hash").and_then(Value::as_str) {
                if seen.insert(hash.to_ascii_lowercase()) {
                    hashes.push(hash.to_string());
                }
            }
        }

        let candidate_transactions = hashes.len();
        let truncated_by_tx_cap = hashes.len() > MAX_CANDIDATE_TRANSACTIONS;
        hashes.truncate(MAX_CANDIDATE_TRANSACTIONS);

        let this = self.clone();
        let wallet_owned = wallet.to_string();
        let results: Vec<Result<(RawWalletTransaction, Vec<String>), String>> =
            stream::iter(hashes.into_iter().map(move |hash| {
                let client = this.clone();
                let wallet = wallet_owned.clone();
                async move { client.reconstruct_transaction(&wallet, &hash).await }
            }))
            .buffer_unordered(CONCURRENT_TX_FETCHES)
            .collect()
            .await;

        let mut transactions = Vec::new();
        let mut notes = Vec::new();

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

        let truncated = !transfer_pages.complete || truncated_by_tx_cap;
        if truncated_by_tx_cap {
            notes.push(format!(
                "Wallet has more than {MAX_CANDIDATE_TRANSACTIONS} target-token transactions; this request intentionally capped the backfill."
            ));
        }

        let complete = !truncated
            && transactions.len() == candidate_transactions
            && notes.iter().all(|note| !note.starts_with("Incomplete tx "));

        RawHistory {
            coverage: HistoryCoverage {
                source: "Robinhood Blockscout address token transfers + transaction traces"
                    .to_string(),
                complete,
                pages_read: transfer_pages.pages_read,
                candidate_transactions,
                reconstructed_transactions: transactions.len(),
                truncated,
                notes,
            },
            transactions,
        }
    }

    async fn reconstruct_transaction(
        &self,
        wallet: &str,
        hash: &str,
    ) -> Result<(RawWalletTransaction, Vec<String>), String> {
        let tx_url = format!("{}/api/v2/transactions/{hash}", self.blockscout_url);
        let internal_url =
            format!("{}/api/v2/transactions/{hash}/internal-transactions", self.blockscout_url);

        let tx_request = self.get_json(&tx_url);
        let internal_request = Url::parse(&internal_url)
            .map_err(|error| format!("Could not construct internal-tx URL for {hash}: {error}"))?;

        let (tx_result, internal_pages) = tokio::join!(
            tx_request,
            self.fetch_pages(internal_request, MAX_TX_SUBRESOURCE_PAGES)
        );

        let tx = tx_result.map_err(|error| format!("Could not fetch tx {hash}: {error}"))?;
        let timestamp = tx
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_timestamp)
            .ok_or_else(|| format!("Transaction {hash} did not contain a parseable timestamp"))?;

        let mut notes = Vec::new();
        if !internal_pages.complete {
            notes.push(format!(
                "Incomplete tx {hash}: internal-transaction pagination exceeded the safety cap."
            ));
        }

        let token_transfers = if tx
            .get("token_transfers_overflow")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let full_url = Url::parse_with_params(
                &format!(
                    "{}/api/v2/transactions/{hash}/token-transfers",
                    self.blockscout_url
                ),
                &[("type", "ERC-20")],
            )
            .map_err(|error| format!("Could not construct token-transfer URL for {hash}: {error}"))?;

            let pages = self
                .fetch_pages(full_url, MAX_TX_SUBRESOURCE_PAGES)
                .await;

            if !pages.complete {
                notes.push(format!(
                    "Incomplete tx {hash}: token-transfer pagination exceeded the safety cap."
                ));
            }

            pages.items
        } else {
            tx.get("token_transfers")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        };

        let mut asset_deltas: HashMap<String, Decimal> = HashMap::new();

        for transfer in token_transfers {
            if transfer.get("token_type").and_then(Value::as_str) != Some("ERC-20") {
                continue;
            }

            let Some(token_address) = token_address(&transfer) else {
                continue;
            };
            let Some(quantity) = transfer_quantity(&transfer) else {
                notes.push(format!(
                    "Incomplete tx {hash}: an ERC-20 transfer amount could not be normalized."
                ));
                continue;
            };

            let from = address_hash(transfer.get("from"));
            let to = address_hash(transfer.get("to"));

            let entry = asset_deltas
                .entry(token_address.to_ascii_lowercase())
                .or_insert(Decimal::ZERO);

            if from.is_some_and(|address| address.eq_ignore_ascii_case(wallet)) {
                *entry -= quantity;
            }
            if to.is_some_and(|address| address.eq_ignore_ascii_case(wallet)) {
                *entry += quantity;
            }
        }

        let native_delta = native_wallet_delta(wallet, &tx, &internal_pages.items);
        if native_delta != Decimal::ZERO {
            asset_deltas.insert("ETH".to_string(), native_delta);
        }

        let fee_quantity = transaction_fee_quantity(wallet, &tx);

        let assets = asset_deltas
            .into_iter()
            .filter(|(_, delta)| *delta != Decimal::ZERO)
            .map(|(asset_id, delta)| RawAssetFlow { asset_id, delta })
            .collect();

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

    async fn fetch_pages(&self, mut url: Url, max_pages: usize) -> PageResult {
        let mut items = Vec::new();
        let mut pages_read = 0usize;
        let mut complete = true;

        loop {
            if pages_read >= max_pages {
                complete = false;
                break;
            }

            let body = match self.get_json(url.as_str()).await {
                Ok(body) => body,
                Err(error) => {
                    complete = false;
                    items.push(serde_json::json!({
                        "_water_fetch_error": error
                    }));
                    break;
                }
            };

            pages_read += 1;

            if let Some(rows) = body.get("items").and_then(Value::as_array) {
                items.extend(rows.iter().cloned());
            }

            let Some(next) = body.get("next_page_params") else {
                break;
            };
            if next.is_null() {
                break;
            }

            let Some(params) = next.as_object() else {
                complete = false;
                break;
            };
            if params.is_empty() {
                break;
            }

            let mut next_url = url.clone();
            {
                let mut pairs = next_url.query_pairs_mut();
                for (key, value) in params {
                    if let Some(value) = query_value(value) {
                        pairs.append_pair(key, &value);
                    }
                }
            }
            url = next_url;
        }

        PageResult {
            items,
            pages_read,
            complete,
        }
    }

    async fn get_json(&self, url: &str) -> Result<Value, String> {
        let response = self
            .http
            .get(url)
            .header("Accept", "application/json")
            .header("User-Agent", "water/0.1")
            .send()
            .await
            .map_err(|error| error.to_string())?;

        let status = response.status();
        let body = response.text().await.map_err(|error| error.to_string())?;

        if !status.is_success() {
            return Err(format!("HTTP {}: {}", status.as_u16(), body));
        }

        serde_json::from_str(&body).map_err(|error| error.to_string())
    }
}

#[derive(Debug)]
struct PageResult {
    items: Vec<Value>,
    pages_read: usize,
    complete: bool,
}

fn empty_history(detail: String) -> RawHistory {
    RawHistory {
        transactions: Vec::new(),
        coverage: HistoryCoverage {
            source: "Robinhood Blockscout".to_string(),
            complete: false,
            pages_read: 0,
            candidate_transactions: 0,
            reconstructed_transactions: 0,
            truncated: false,
            notes: vec![detail],
        },
    }
}

fn parse_timestamp(value: &str) -> Option<u64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .and_then(|timestamp| u64::try_from(timestamp.timestamp()).ok())
}

fn address_hash(value: Option<&Value>) -> Option<&str> {
    value?
        .get("hash")
        .and_then(Value::as_str)
        .or_else(|| value?.as_str())
}

fn token_address(transfer: &Value) -> Option<&str> {
    let token = transfer.get("token")?;

    token
        .get("address")
        .and_then(Value::as_str)
        .or_else(|| token.get("address_hash").and_then(Value::as_str))
}

fn transfer_quantity(transfer: &Value) -> Option<Decimal> {
    let total = transfer.get("total")?;
    let raw = total.get("value")?.as_str()?;
    let decimals = total
        .get("decimals")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<u32>().ok())
        .or_else(|| {
            transfer
                .pointer("/token/decimals")
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<u32>().ok())
        })?;

    scaled_decimal(raw, decimals)
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

fn native_wallet_delta(wallet: &str, tx: &Value, internal: &[Value]) -> Decimal {
    let mut delta = Decimal::ZERO;

    if let Some(value) = tx.get("value").and_then(Value::as_str).and_then(wei_to_eth) {
        let from = address_hash(tx.get("from"));
        let to = address_hash(tx.get("to"));

        if from.is_some_and(|address| address.eq_ignore_ascii_case(wallet)) {
            delta -= value;
        }
        if to.is_some_and(|address| address.eq_ignore_ascii_case(wallet)) {
            delta += value;
        }
    }

    for item in internal {
        if !item
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }

        let Some(value) = item
            .get("value")
            .and_then(Value::as_str)
            .and_then(wei_to_eth)
        else {
            continue;
        };

        let from = address_hash(item.get("from"));
        let to = address_hash(item.get("to"));

        if from.is_some_and(|address| address.eq_ignore_ascii_case(wallet)) {
            delta -= value;
        }
        if to.is_some_and(|address| address.eq_ignore_ascii_case(wallet)) {
            delta += value;
        }
    }

    delta
}

fn transaction_fee_quantity(wallet: &str, tx: &Value) -> Option<Decimal> {
    if !address_hash(tx.get("from")).is_some_and(|address| address.eq_ignore_ascii_case(wallet)) {
        return None;
    }

    tx.pointer("/fee/value")
        .and_then(Value::as_str)
        .and_then(wei_to_eth)
}

fn wei_to_eth(value: &str) -> Option<Decimal> {
    scaled_decimal(value, 18)
}

fn query_value(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scales_raw_erc20_values_without_parsing_huge_integer_first() {
        assert_eq!(
            scaled_decimal("1000000000000000000000000000000", 18),
            Some(Decimal::from(1_000_000_000_000i64))
        );
    }

    #[test]
    fn computes_wallet_erc20_delta_from_transfer_direction() {
        let wallet = "0x1111111111111111111111111111111111111111";
        let transfer = json!({
            "token_type": "ERC-20",
            "from": {"hash": "0x2222222222222222222222222222222222222222"},
            "to": {"hash": wallet},
            "token": {
                "address": "0x3333333333333333333333333333333333333333",
                "decimals": "18"
            },
            "total": {
                "value": "2500000000000000000",
                "decimals": "18"
            }
        });

        assert_eq!(transfer_quantity(&transfer), Some(Decimal::new(25, 1)));
        assert_eq!(
            address_hash(transfer.get("to")),
            Some(wallet)
        );
    }

    #[test]
    fn native_value_and_internal_refund_are_net_of_gas() {
        let wallet = "0x1111111111111111111111111111111111111111";
        let tx = json!({
            "from": {"hash": wallet},
            "to": {"hash": "0x2222222222222222222222222222222222222222"},
            "value": "1000000000000000000",
            "fee": {"value": "1000000000000000"}
        });
        let internal = vec![json!({
            "success": true,
            "from": {"hash": "0x2222222222222222222222222222222222222222"},
            "to": {"hash": wallet},
            "value": "100000000000000000"
        })];

        assert_eq!(
            native_wallet_delta(wallet, &tx, &internal),
            Decimal::new(-9, 1)
        );
        assert_eq!(
            transaction_fee_quantity(wallet, &tx),
            Some(Decimal::new(1, 3))
        );
    }
}
