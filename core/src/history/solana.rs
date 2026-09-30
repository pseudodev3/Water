use super::{HolderCandidate, HistoryCoverage, RawAssetFlow, RawHistory, RawWalletTransaction};
use futures::{stream, StreamExt};
use reqwest::Client;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

const MAX_SIGNATURE_PAGES_PER_ADDRESS: usize = 5;
const SIGNATURE_PAGE_SIZE: usize = 1000;
const MAX_CANDIDATE_TRANSACTIONS: usize = 250;
const CONCURRENT_TX_FETCHES: usize = 8;
const LAMPORTS_PER_SOL: i64 = 1_000_000_000;

#[derive(Clone)]
pub struct SolanaHistoryClient {
    http: Client,
    rpc_url: String,
}

impl SolanaHistoryClient {
    pub fn new(http: Client, rpc_url: String) -> Self {
        Self { http, rpc_url }
    }


    pub async fn top_current_holders(
        &self,
        target_mint: &str,
        limit: usize,
    ) -> Result<Vec<HolderCandidate>, String> {
        let largest = self
            .rpc("getTokenLargestAccounts", json!([target_mint]))
            .await?;

        let rows = largest
            .get("value")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let account_addresses: Vec<String> = rows
            .iter()
            .filter_map(|row| row.get("address").and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .collect();

        if account_addresses.is_empty() {
            return Ok(Vec::new());
        }

        let infos = self
            .rpc(
                "getMultipleAccounts",
                json!([
                    account_addresses,
                    {"encoding": "jsonParsed", "commitment": "confirmed"}
                ]),
            )
            .await?;

        let account_infos = infos
            .get("value")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut by_owner: HashMap<String, Decimal> = HashMap::new();

        for (row, info) in rows.iter().zip(account_infos.iter()) {
            let Some(owner) = info
                .pointer("/data/parsed/info/owner")
                .and_then(Value::as_str)
            else {
                continue;
            };

            let quantity = row
                .get("uiAmountString")
                .and_then(Value::as_str)
                .and_then(|value| Decimal::from_str(value).ok())
                .or_else(|| {
                    let raw = row.get("amount")?.as_str()?;
                    let decimals = row.get("decimals")?.as_u64()? as u32;
                    scaled_decimal(raw, decimals)
                });

            if let Some(quantity) = quantity {
                *by_owner.entry(owner.to_string()).or_insert(Decimal::ZERO) += quantity;
            }
        }

        let mut owners: Vec<(String, Decimal)> = by_owner.into_iter().collect();
        owners.sort_by(|left, right| right.1.cmp(&left.1));

        Ok(owners
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
        target_mint: &str,
    ) -> RawHistory {
        let token_accounts = self
            .target_token_accounts(wallet, target_mint)
            .await
            .unwrap_or_default();

        let mut observed_addresses = vec![wallet.to_string()];
        observed_addresses.extend(token_accounts.addresses.iter().cloned());

        let mut signatures = HashSet::new();
        let mut pages_read = 0usize;
        let mut signature_scan_complete = true;
        let mut notes = vec![
            "Solana standard RPC can only discover the wallet plus currently discoverable target-token accounts. Previously closed ATAs may be absent; basis coverage is authoritative, not a claim of perfect wallet history."
                .to_string(),
        ];

        for address in &observed_addresses {
            let scan = self.signatures_for_address(address).await;
            pages_read += scan.pages_read;
            signature_scan_complete &= scan.complete;

            for signature in scan.signatures {
                signatures.insert(signature);
            }
        }

        let candidate_transactions = signatures.len();
        let mut signatures: Vec<String> = signatures.into_iter().collect();
        let truncated_by_tx_cap = signatures.len() > MAX_CANDIDATE_TRANSACTIONS;
        signatures.truncate(MAX_CANDIDATE_TRANSACTIONS);

        if truncated_by_tx_cap {
            notes.push(format!(
                "More than {MAX_CANDIDATE_TRANSACTIONS} candidate transactions were discovered; this request capped the backfill."
            ));
        }

        let this = self.clone();
        let wallet_owned = wallet.to_string();
        let results: Vec<Result<(RawWalletTransaction, Vec<String>), String>> =
            stream::iter(signatures.into_iter().map(move |signature| {
                let client = this.clone();
                let wallet = wallet_owned.clone();
                async move { client.reconstruct_transaction(&wallet, &signature).await }
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

        let truncated = truncated_by_tx_cap || !signature_scan_complete;
        let complete = false; // See closed-ATA limitation above.
        let reconstructed_transactions = transactions.len();

        RawHistory {
            transactions,
            coverage: HistoryCoverage {
                source: "Solana JSON-RPC wallet + current target-token account history".to_string(),
                complete,
                pages_read,
                candidate_transactions,
                reconstructed_transactions,
                observed_current_quantity: token_accounts.current_quantity,
                truncated,
                notes,
            },
        }
    }

    async fn target_token_accounts(
        &self,
        wallet: &str,
        target_mint: &str,
    ) -> Result<TokenAccountsSnapshot, String> {
        let result = self
            .rpc(
                "getTokenAccountsByOwner",
                json!([
                    wallet,
                    {"mint": target_mint},
                    {"encoding": "jsonParsed"}
                ]),
            )
            .await?;

        let rows = result
            .get("value")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let addresses = rows
            .iter()
            .filter_map(|row| row.get("pubkey").and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .collect();

        let current_quantity = rows.iter().try_fold(Decimal::ZERO, |total, row| {
            let token_amount = row.pointer("/account/data/parsed/info/tokenAmount")?;
            let raw = token_amount.get("amount")?.as_str()?;
            let decimals = token_amount.get("decimals")?.as_u64()? as u32;
            let quantity = scaled_decimal(raw, decimals)?;
            Some(total + quantity)
        });

        Ok(TokenAccountsSnapshot {
            addresses,
            current_quantity,
        })
    }

    async fn signatures_for_address(&self, address: &str) -> SignatureScan {
        let mut signatures = Vec::new();
        let mut before: Option<String> = None;
        let mut pages_read = 0usize;
        let mut complete = true;

        loop {
            if pages_read >= MAX_SIGNATURE_PAGES_PER_ADDRESS {
                complete = false;
                break;
            }

            let mut config = serde_json::Map::new();
            config.insert("limit".to_string(), json!(SIGNATURE_PAGE_SIZE));
            if let Some(cursor) = &before {
                config.insert("before".to_string(), json!(cursor));
            }

            let result = match self
                .rpc(
                    "getSignaturesForAddress",
                    json!([address, Value::Object(config)]),
                )
                .await
            {
                Ok(result) => result,
                Err(_) => {
                    complete = false;
                    break;
                }
            };

            pages_read += 1;
            let rows = result.as_array().cloned().unwrap_or_default();

            if rows.is_empty() {
                break;
            }

            for row in &rows {
                if row.get("err").is_some_and(|value| !value.is_null()) {
                    continue;
                }

                if let Some(signature) = row.get("signature").and_then(Value::as_str) {
                    signatures.push(signature.to_string());
                }
            }

            if rows.len() < SIGNATURE_PAGE_SIZE {
                break;
            }

            before = rows
                .last()
                .and_then(|row| row.get("signature"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);

            if before.is_none() {
                complete = false;
                break;
            }
        }

        SignatureScan {
            signatures,
            pages_read,
            complete,
        }
    }

    async fn reconstruct_transaction(
        &self,
        wallet: &str,
        signature: &str,
    ) -> Result<(RawWalletTransaction, Vec<String>), String> {
        let tx = self
            .rpc(
                "getTransaction",
                json!([
                    signature,
                    {
                        "encoding": "jsonParsed",
                        "commitment": "confirmed",
                        "maxSupportedTransactionVersion": 0
                    }
                ]),
            )
            .await
            .map_err(|error| format!("Could not fetch Solana tx {signature}: {error}"))?;

        if tx.is_null() {
            return Err(format!(
                "Could not reconstruct Solana tx {signature}: RPC returned null, usually indicating unavailable archival history."
            ));
        }

        let timestamp = tx
            .get("blockTime")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("Solana tx {signature} has no blockTime"))?;

        let meta = tx
            .get("meta")
            .ok_or_else(|| format!("Solana tx {signature} has no meta"))?;

        let mut notes = Vec::new();
        let asset_deltas = owner_token_deltas(wallet, meta);

        let mut assets: Vec<RawAssetFlow> = asset_deltas
            .into_iter()
            .filter(|(_, delta)| *delta != Decimal::ZERO)
            .map(|(asset_id, delta)| RawAssetFlow { asset_id, delta })
            .collect();

        if let Some(native_delta) = native_sol_delta_excluding_fee(wallet, &tx) {
            if native_delta != Decimal::ZERO {
                assets.push(RawAssetFlow {
                    asset_id: "SOL".to_string(),
                    delta: native_delta,
                });

                notes.push(format!(
                    "Solana tx {signature}: native SOL delta is fee-adjusted but can still include account-rent creation/closure effects; token-to-token quote deltas are higher-confidence."
                ));
            }
        }

        let fee_lamports = meta.get("fee").and_then(Value::as_u64);
        let wallet_is_fee_payer = account_key(&tx, 0)
            .is_some_and(|address| address == wallet);

        let fee_quantity = if wallet_is_fee_payer {
            fee_lamports.map(|lamports| {
                Decimal::from(lamports) / Decimal::from(LAMPORTS_PER_SOL)
            })
        } else {
            None
        };

        Ok((
            RawWalletTransaction {
                tx_id: signature.to_string(),
                timestamp,
                network_fee_asset_id: fee_quantity.map(|_| "SOL".to_string()),
                network_fee_quantity: fee_quantity,
                assets,
            },
            notes,
        ))
    }

    async fn rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params
        });

        let response = self
            .http
            .post(&self.rpc_url)
            .json(&payload)
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

#[derive(Debug, Default)]
struct TokenAccountsSnapshot {
    addresses: Vec<String>,
    current_quantity: Option<Decimal>,
}

#[derive(Debug)]
struct SignatureScan {
    signatures: Vec<String>,
    pages_read: usize,
    complete: bool,
}

fn owner_token_deltas(wallet: &str, meta: &Value) -> HashMap<String, Decimal> {
    let mut pre = HashMap::<String, Decimal>::new();
    let mut post = HashMap::<String, Decimal>::new();

    collect_owner_balances(
        wallet,
        meta.get("preTokenBalances").and_then(Value::as_array),
        &mut pre,
    );
    collect_owner_balances(
        wallet,
        meta.get("postTokenBalances").and_then(Value::as_array),
        &mut post,
    );

    let mints: HashSet<String> = pre.keys().chain(post.keys()).cloned().collect();

    mints
        .into_iter()
        .map(|mint| {
            let before = pre.get(&mint).copied().unwrap_or(Decimal::ZERO);
            let after = post.get(&mint).copied().unwrap_or(Decimal::ZERO);
            (mint, after - before)
        })
        .collect()
}

fn collect_owner_balances(
    wallet: &str,
    rows: Option<&Vec<Value>>,
    destination: &mut HashMap<String, Decimal>,
) {
    let Some(rows) = rows else {
        return;
    };

    for row in rows {
        if row.get("owner").and_then(Value::as_str) != Some(wallet) {
            continue;
        }

        let Some(mint) = row.get("mint").and_then(Value::as_str) else {
            continue;
        };
        let Some(amount) = ui_token_raw_amount(row) else {
            continue;
        };

        *destination.entry(mint.to_string()).or_insert(Decimal::ZERO) += amount;
    }
}

fn ui_token_raw_amount(row: &Value) -> Option<Decimal> {
    let raw = row
        .pointer("/uiTokenAmount/amount")
        .and_then(Value::as_str)?;
    let decimals = row
        .pointer("/uiTokenAmount/decimals")
        .and_then(Value::as_u64)? as u32;

    scaled_decimal(raw, decimals)
}

fn native_sol_delta_excluding_fee(wallet: &str, tx: &Value) -> Option<Decimal> {
    let keys = tx.pointer("/transaction/message/accountKeys")?.as_array()?;
    let wallet_index = keys.iter().position(|key| {
        key.as_str()
            .map(|value| value == wallet)
            .or_else(|| {
                key.get("pubkey")
                    .and_then(Value::as_str)
                    .map(|value| value == wallet)
            })
            .unwrap_or(false)
    })?;

    let pre = tx
        .pointer("/meta/preBalances")?
        .as_array()?
        .get(wallet_index)?
        .as_u64()?;
    let post = tx
        .pointer("/meta/postBalances")?
        .as_array()?
        .get(wallet_index)?
        .as_u64()?;

    let mut delta_lamports = i128::from(post) - i128::from(pre);

    if wallet_index == 0 {
        if let Some(fee) = tx.pointer("/meta/fee").and_then(Value::as_u64) {
            delta_lamports += i128::from(fee);
        }
    }

    let numerator = Decimal::from_i128_with_scale(delta_lamports, 0);
    Some(numerator / Decimal::from(LAMPORTS_PER_SOL))
}

fn account_key(tx: &Value, index: usize) -> Option<&str> {
    let key = tx
        .pointer("/transaction/message/accountKeys")?
        .as_array()?
        .get(index)?;

    key.as_str()
        .or_else(|| key.get("pubkey").and_then(Value::as_str))
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
    fn owner_deltas_capture_new_and_closed_token_accounts() {
        let wallet = "WALLET";
        let meta = json!({
            "preTokenBalances": [
                {
                    "mint": "TOKEN",
                    "owner": wallet,
                    "uiTokenAmount": {"amount": "1000000", "decimals": 6}
                }
            ],
            "postTokenBalances": [
                {
                    "mint": "USDC",
                    "owner": wallet,
                    "uiTokenAmount": {"amount": "2000000", "decimals": 6}
                }
            ]
        });

        let deltas = owner_token_deltas(wallet, &meta);

        assert_eq!(deltas.get("TOKEN"), Some(&Decimal::from(-1)));
        assert_eq!(deltas.get("USDC"), Some(&Decimal::from(2)));
    }

    #[test]
    fn fee_adjustment_keeps_network_fee_out_of_sol_quote_delta() {
        let wallet = "WALLET";
        let tx = json!({
            "transaction": {
                "message": {
                    "accountKeys": [
                        {"pubkey": wallet},
                        {"pubkey": "ROUTER"}
                    ]
                }
            },
            "meta": {
                "preBalances": [2_000_000_000u64, 0],
                "postBalances": [899_995_000u64, 0],
                "fee": 5_000u64
            }
        });

        assert_eq!(
            native_sol_delta_excluding_fee(wallet, &tx),
            Some(Decimal::new(-11, 1))
        );
    }
}
