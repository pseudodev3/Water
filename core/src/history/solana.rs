use super::{HolderCandidate, HistoryCoverage, RawAssetFlow, RawHistory, RawWalletTransaction};
use curve25519_dalek::edwards::CompressedEdwardsY;
use futures::{stream, StreamExt};
use reqwest::Client;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

const MAX_SIGNATURE_PAGES_PER_ADDRESS: usize = 5;
const SIGNATURE_PAGE_SIZE: usize = 1000;
const MAX_CANDIDATE_TRANSACTIONS: usize = 250;
const MAX_DISCOVERY_ROUNDS: usize = 4;
const MAX_DISCOVERED_TOKEN_ACCOUNTS: usize = 32;
const CONCURRENT_TX_FETCHES: usize = 8;
const LAMPORTS_PER_SOL: i64 = 1_000_000_000;
const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";
const GET_TOKEN_LARGEST_ACCOUNTS_LIMIT: usize = 20;
const DEFAULT_FALLBACK_SOLANA_RPC: &str = "https://solana-rpc.publicnode.com";

#[derive(Clone)]
pub struct SolanaHistoryClient {
    http: Client,
    rpc_url: String,
    fallback_rpc_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SolanaWalletHolderSet {
    pub holders: Vec<HolderCandidate>,
    pub scanned_token_accounts: usize,
    pub excluded_program_authorities: usize,
    pub excluded_program_quantity: Decimal,
    pub complete_for_requested: bool,
}

impl SolanaHistoryClient {
    pub fn new(http: Client, rpc_url: String) -> Self {
        let fallback_rpc_url = (rpc_url.trim_end_matches('/') != DEFAULT_FALLBACK_SOLANA_RPC)
            .then(|| DEFAULT_FALLBACK_SOLANA_RPC.to_string());

        Self {
            http,
            rpc_url,
            fallback_rpc_url,
        }
    }

    pub fn with_fallback(
        http: Client,
        rpc_url: String,
        fallback_rpc_url: String,
    ) -> Self {
        Self {
            http,
            rpc_url,
            fallback_rpc_url: Some(fallback_rpc_url),
        }
    }

    pub async fn top_current_holders(
        &self,
        target_mint: &str,
        limit: usize,
    ) -> Result<Vec<HolderCandidate>, String> {
        Ok(self
            .top_wallet_holders(target_mint, limit)
            .await?
            .holders)
    }

    pub async fn top_wallet_holders(
        &self,
        target_mint: &str,
        limit: usize,
    ) -> Result<SolanaWalletHolderSet, String> {
        let largest = self
            .rpc("getTokenLargestAccounts", json!([target_mint]))
            .await?;

        let rows = largest
            .get("value")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let scanned_token_accounts = rows.len();
        let account_addresses: Vec<String> = rows
            .iter()
            .filter_map(|row| row.get("address").and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .collect();

        if account_addresses.is_empty() {
            return Ok(SolanaWalletHolderSet {
                holders: Vec::new(),
                scanned_token_accounts: 0,
                excluded_program_authorities: 0,
                excluded_program_quantity: Decimal::ZERO,
                complete_for_requested: true,
            });
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

        let mut by_authority: HashMap<String, Decimal> = HashMap::new();

        for (row, info) in rows.iter().zip(account_infos.iter()) {
            let Some(authority) = info
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
                *by_authority
                    .entry(authority.to_string())
                    .or_insert(Decimal::ZERO) += quantity;
            }
        }

        let mut authorities: Vec<(String, Decimal)> = by_authority.into_iter().collect();
        authorities.sort_by(|left, right| right.1.cmp(&left.1));

        let authority_addresses: Vec<String> =
            authorities.iter().map(|(address, _)| address.clone()).collect();

        let authority_infos = if authority_addresses.is_empty() {
            Vec::new()
        } else {
            self.rpc(
                "getMultipleAccounts",
                json!([
                    authority_addresses,
                    {"encoding": "base64", "commitment": "confirmed"}
                ]),
            )
            .await?
            .get("value")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        };

        let mut holders = Vec::new();
        let mut excluded_program_authorities = 0usize;
        let mut excluded_program_quantity = Decimal::ZERO;

        for ((authority, quantity), info) in authorities.into_iter().zip(
            authority_infos
                .into_iter()
                .chain(std::iter::repeat(Value::Null)),
        ) {
            let wallet_like = is_on_curve_pubkey(&authority)
                && authority_account_is_wallet_like(&info);

            if !wallet_like {
                excluded_program_authorities += 1;
                excluded_program_quantity += quantity;
                continue;
            }

            if holders.len() < limit {
                holders.push(HolderCandidate {
                    wallet: authority,
                    current_quantity: quantity,
                    source_rank: holders.len() + 1,
                });
            }
        }

        let complete_for_requested = holders.len() >= limit
            || scanned_token_accounts < GET_TOKEN_LARGEST_ACCOUNTS_LIMIT;

        Ok(SolanaWalletHolderSet {
            holders,
            scanned_token_accounts,
            excluded_program_authorities,
            excluded_program_quantity,
            complete_for_requested,
        })
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

        let mut discovered_accounts: HashSet<String> =
            token_accounts.addresses.iter().cloned().collect();
        let mut pending_addresses = vec![wallet.to_string()];
        pending_addresses.extend(token_accounts.addresses.iter().cloned());

        let mut scanned_addresses = HashSet::new();
        let mut signatures: HashMap<String, u64> = HashMap::new();
        let mut pages_read = 0usize;
        let mut signature_scan_complete = true;
        let mut notes = Vec::new();
        let mut discovery_rounds = 0usize;
        let mut discovery_stable = false;

        while !pending_addresses.is_empty() && discovery_rounds < MAX_DISCOVERY_ROUNDS {
            discovery_rounds += 1;
            let addresses = std::mem::take(&mut pending_addresses);

            for address in addresses {
                if !scanned_addresses.insert(address.clone()) {
                    continue;
                }

                let scan = self.signatures_for_address(&address).await;
                pages_read += scan.pages_read;
                signature_scan_complete &= scan.complete;

                for record in scan.signatures {
                    signatures
                        .entry(record.signature)
                        .and_modify(|timestamp| *timestamp = (*timestamp).min(record.block_time))
                        .or_insert(record.block_time);
                }
            }

            let mut ordered: Vec<(String, u64)> = signatures
                .iter()
                .map(|(signature, block_time)| (signature.clone(), *block_time))
                .collect();
            ordered.sort_by_key(|(_, block_time)| *block_time);
            ordered.truncate(MAX_CANDIDATE_TRANSACTIONS);

            let this = self.clone();
            let discovery_results: Vec<Result<Value, String>> =
                stream::iter(ordered.into_iter().map(move |(signature, _)| {
                    let client = this.clone();
                    async move { client.fetch_transaction(&signature).await }
                }))
                .buffer_unordered(CONCURRENT_TX_FETCHES)
                .collect()
                .await;

            let before = discovered_accounts.len();

            for tx in discovery_results.into_iter().flatten() {
                for account in discover_target_token_accounts(wallet, target_mint, &tx) {
                    if discovered_accounts.len() >= MAX_DISCOVERED_TOKEN_ACCOUNTS {
                        break;
                    }

                    if discovered_accounts.insert(account.clone())
                        && !scanned_addresses.contains(&account)
                    {
                        pending_addresses.push(account);
                    }
                }
            }

            if discovered_accounts.len() == before {
                discovery_stable = true;
                break;
            }

            if discovered_accounts.len() >= MAX_DISCOVERED_TOKEN_ACCOUNTS {
                notes.push(format!(
                    "Solana token-account discovery hit the {MAX_DISCOVERED_TOKEN_ACCOUNTS}-account safety cap."
                ));
                break;
            }
        }

        notes.push(format!(
            "Solana history scanned the wallet plus {} discovered/current target-token account(s) across {discovery_rounds} discovery round(s).",
            discovered_accounts.len()
        ));

        if discovery_stable {
            notes.push(
                "Closed target-token accounts referenced by wallet history were recursively discovered and included."
                    .to_string(),
            );
        } else {
            notes.push(
                "Token-account discovery did not reach a stable fixed point before a safety cap; history remains explicitly partial."
                    .to_string(),
            );
        }

        let candidate_transactions = signatures.len();
        let mut signatures: Vec<(String, u64)> = signatures.into_iter().collect();
        signatures.sort_by_key(|(_, block_time)| *block_time);
        let truncated_by_tx_cap = signatures.len() > MAX_CANDIDATE_TRANSACTIONS;
        signatures.truncate(MAX_CANDIDATE_TRANSACTIONS);
        let signatures: Vec<String> = signatures
            .into_iter()
            .map(|(signature, _)| signature)
            .collect();

        if truncated_by_tx_cap {
            notes.push(format!(
                "More than {MAX_CANDIDATE_TRANSACTIONS} candidate transactions were discovered; Water retained the earliest observed candidates and marks the reconstruction truncated."
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

        let truncated = truncated_by_tx_cap
            || !signature_scan_complete
            || !discovery_stable
            || discovered_accounts.len() >= MAX_DISCOVERED_TOKEN_ACCOUNTS;
        let reconstructed_transactions = transactions.len();

        RawHistory {
            transactions,
            coverage: HistoryCoverage {
                source: "Solana JSON-RPC recursive wallet + historical target-token-account discovery"
                    .to_string(),
                // Standard RPC is much tighter now, but we reserve 'complete' for an
                // indexed history source that can guarantee all historical token accounts.
                complete: false,
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
                    let block_time = row
                        .get("blockTime")
                        .and_then(Value::as_u64)
                        .unwrap_or(u64::MAX);
                    signatures.push(SignatureRecord {
                        signature: signature.to_string(),
                        block_time,
                    });
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

    async fn fetch_transaction(&self, signature: &str) -> Result<Value, String> {
        self.rpc(
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
        .map_err(|error| format!("Could not fetch Solana tx {signature}: {error}"))
    }

    async fn reconstruct_transaction(
        &self,
        wallet: &str,
        signature: &str,
    ) -> Result<(RawWalletTransaction, Vec<String>), String> {
        let tx = self.fetch_transaction(signature).await?;

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
        match self.rpc_on(&self.rpc_url, method, params.clone()).await {
            Ok(value) => Ok(value),
            Err(primary_error) => {
                let Some(fallback) = self
                    .fallback_rpc_url
                    .as_ref()
                    .filter(|url| *url != &self.rpc_url)
                else {
                    return Err(primary_error);
                };

                self.rpc_on(fallback, method, params)
                    .await
                    .map_err(|fallback_error| {
                        format!(
                            "primary RPC failed ({primary_error}); fallback RPC failed ({fallback_error})"
                        )
                    })
            }
        }
    }

    async fn rpc_on(
        &self,
        rpc_url: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params
        });

        let mut last_error = None;

        for attempt in 0..4 {
            match self.http.post(rpc_url).json(&payload).send().await {
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

                    if status.as_u16() == 429 {
                        last_error = Some(format!("HTTP 429: {body}"));
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

        Err(last_error.unwrap_or_else(|| "Solana RPC request failed.".to_string()))
    }
}

#[derive(Debug, Default)]
struct TokenAccountsSnapshot {
    addresses: Vec<String>,
    current_quantity: Option<Decimal>,
}

#[derive(Debug)]
struct SignatureRecord {
    signature: String,
    block_time: u64,
}

#[derive(Debug)]
struct SignatureScan {
    signatures: Vec<SignatureRecord>,
    pages_read: usize,
    complete: bool,
}



fn is_on_curve_pubkey(address: &str) -> bool {
    let Ok(bytes) = bs58::decode(address).into_vec() else {
        return false;
    };
    let Ok(bytes) = <[u8; 32]>::try_from(bytes.as_slice()) else {
        return false;
    };

    CompressedEdwardsY(bytes).decompress().is_some()
}

fn authority_account_is_wallet_like(info: &Value) -> bool {
    if info.is_null() {
        return true;
    }

    if info
        .get("executable")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return false;
    }

    info.get("owner")
        .and_then(Value::as_str)
        .map(|owner| owner == SYSTEM_PROGRAM)
        .unwrap_or(true)
}

fn discover_target_token_accounts(
    wallet: &str,
    target_mint: &str,
    tx: &Value,
) -> HashSet<String> {
    let mut discovered = HashSet::new();
    let keys = tx
        .pointer("/transaction/message/accountKeys")
        .and_then(Value::as_array);

    for path in ["/meta/preTokenBalances", "/meta/postTokenBalances"] {
        let Some(rows) = tx.pointer(path).and_then(Value::as_array) else {
            continue;
        };

        for row in rows {
            if row.get("owner").and_then(Value::as_str) != Some(wallet)
                || row.get("mint").and_then(Value::as_str) != Some(target_mint)
            {
                continue;
            }

            let Some(index) = row.get("accountIndex").and_then(Value::as_u64) else {
                continue;
            };
            let Some(key) = keys
                .and_then(|keys| keys.get(index as usize))
                .and_then(|key| {
                    key.as_str()
                        .or_else(|| key.get("pubkey").and_then(Value::as_str))
                })
            else {
                continue;
            };

            discovered.insert(key.to_string());
        }
    }

    discovered
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
    fn discovers_closed_target_token_account_from_balance_metadata() {
        let wallet = "WALLET";
        let tx = json!({
            "transaction": {
                "message": {
                    "accountKeys": [
                        {"pubkey": wallet},
                        {"pubkey": "OLD_CLOSED_ATA"}
                    ]
                }
            },
            "meta": {
                "preTokenBalances": [
                    {
                        "accountIndex": 1,
                        "mint": "TOKEN",
                        "owner": wallet,
                        "uiTokenAmount": {"amount": "100", "decimals": 0}
                    }
                ],
                "postTokenBalances": []
            }
        });

        let accounts = discover_target_token_accounts(wallet, "TOKEN", &tx);
        assert!(accounts.contains("OLD_CLOSED_ATA"));
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
