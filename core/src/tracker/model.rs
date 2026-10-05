use crate::model::Chain;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

pub const DAY: u64 = 86_400;
pub const POLICY: &str = "wallet-consistency-v1";

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn wallet_key(chain: Chain, address: &str) -> Result<String, String> {
    let address = address.trim();
    let normalized = match chain {
        Chain::Solana => {
            let bytes = bs58::decode(address)
                .into_vec()
                .map_err(|_| "Enter a valid Solana wallet address.")?;
            if bytes.len() != 32 {
                return Err("Solana wallets must decode to 32 bytes.".into());
            }
            address.to_string()
        }
        Chain::Robinhood | Chain::Bnb => {
            if address.len() != 42
                || !address.starts_with("0x")
                || !address[2..].chars().all(|c| c.is_ascii_hexdigit())
            {
                return Err(format!(
                    "Enter a valid {} 0x wallet address.",
                    chain.label()
                ));
            }
            address.to_ascii_lowercase()
        }
    };
    Ok(normalized)
}

pub fn native(chain: Chain) -> &'static str {
    match chain {
        Chain::Solana => "SOL",
        Chain::Robinhood => "ETH",
        Chain::Bnb => "BNB",
    }
}

pub fn quote(chain: Chain, asset: &str) -> bool {
    match chain {
        Chain::Solana => matches!(
            asset,
            "SOL"
                | "So11111111111111111111111111111111111111112"
                | "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
                | "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
        ),
        Chain::Robinhood => {
            asset.eq_ignore_ascii_case("ETH")
                || asset.eq_ignore_ascii_case("0x0bd7d308f8e1639fab988df18a8011f41eacad73")
        }
        Chain::Bnb => {
            asset.eq_ignore_ascii_case("BNB")
                || matches!(
                    asset.to_ascii_lowercase().as_str(),
                    "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c"
                        | "0x55d398326f99059ff775485246999027b3197955"
                        | "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d"
                )
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Source {
    pub name: String,
    pub observed_at: u64,
    pub detail: String,
    pub profile: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Candidate {
    pub chain: Chain,
    pub wallet: String,
    pub discovered_at: u64,
    pub sources: Vec<Source>,
    #[serde(default)]
    pub observed_tokens: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Coverage {
    pub provider: String,
    pub cursor: Option<String>,
    pub head_cursor: Option<String>,
    pub head_cursors: Vec<String>,
    pub public_scan_ranges: Vec<(u64, u64)>,
    pub head_complete: bool,
    pub backfill_started: bool,
    pub backfill_done: bool,
    pub history_complete: bool,
    pub ordering_complete: bool,
    pub balances_reconciled: bool,
    pub execution_account_verified: bool,
    pub fees_complete: bool,
    pub last_collected_at: Option<u64>,
    pub last_state_checked_at: Option<u64>,
    pub oldest_record_at: Option<u64>,
    pub newest_record_at: Option<u64>,
    pub pages: usize,
    pub pending_records: usize,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Delta {
    pub asset: String,
    pub quantity: Decimal,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Transaction {
    pub id: String,
    pub timestamp: u64,
    pub block: u64,
    pub index: Option<u64>,
    pub finalized: bool,
    pub succeeded: bool,
    pub assets: Vec<Delta>,
    pub fee_asset: String,
    pub fee_quantity: Option<Decimal>,
    pub movement_complete: bool,
    pub swap_evidence: bool,
    pub notes: Vec<String>,
    pub counterparties: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record {
    pub id: String,
    pub raw: Value,
    pub transaction: Option<Transaction>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Price {
    pub asset: String,
    pub timestamp: u64,
    pub usd: Decimal,
    pub granularity: String,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Gate {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Window {
    pub start: u64,
    pub end: u64,
    pub realized_usd: Option<Decimal>,
    pub open_change_usd: Option<Decimal>,
    pub fees_usd: Option<Decimal>,
    pub total_usd: Option<Decimal>,
    pub episodes: usize,
    pub tokens: usize,
    pub active_days: usize,
    pub wins: usize,
    pub losses: usize,
    pub profit_factor: Option<Decimal>,
    pub largest_winner_usd: Option<Decimal>,
    pub profit_without_largest_usd: Option<Decimal>,
    pub largest_profit_share: Option<Decimal>,
    pub gates: Vec<Gate>,
    pub qualified: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Position {
    pub asset: String,
    pub quantity: Decimal,
    pub known_cost_usd: Decimal,
    pub basis_coverage: Decimal,
    pub market_value_usd: Option<Decimal>,
    pub realized_usd: Option<Decimal>,
    pub first_acquired_at: Option<u64>,
    pub last_activity_at: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Activity {
    pub tx: String,
    pub timestamp: u64,
    pub kind: String,
    pub asset: Option<String>,
    pub quantity: Option<Decimal>,
    pub quote_asset: Option<String>,
    pub quote_quantity: Option<Decimal>,
    pub value_usd: Option<Decimal>,
    pub finalized: bool,
    pub counterparties: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Analysis {
    pub candidate: Candidate,
    pub analyzed_at: u64,
    pub policy: String,
    pub status: String,
    pub coverage: Coverage,
    pub windows: Vec<Window>,
    pub positions: Vec<Position>,
    pub activity: Vec<Activity>,
    pub unresolved_records: usize,
    pub records: usize,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Snapshot {
    pub candidate: Candidate,
    pub coverage: Coverage,
    pub records: Vec<Record>,
    pub prices: Vec<Price>,
    pub balances: BTreeMap<String, Decimal>,
}

#[derive(Debug, Deserialize)]
pub struct WalletRequest {
    pub chain: Chain,
    pub wallet: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wallet_identity_is_exact_and_chain_specific() {
        let sol = "21rgbFW6sujQovCw3qt6R2EdE97Yzzvk8sSc37Bb72Cm";
        assert_eq!(wallet_key(Chain::Solana, sol).unwrap(), sol);
        assert!(wallet_key(Chain::Solana, "a".repeat(32).as_str()).is_err());
        assert_eq!(
            wallet_key(
                Chain::Robinhood,
                "0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73"
            )
            .unwrap(),
            "0x0bd7d308f8e1639fab988df18a8011f41eacad73"
        );
    }
}
