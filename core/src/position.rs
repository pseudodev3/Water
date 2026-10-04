use crate::{
    config::Config,
    early::HolderBehaviorMetrics,
    history::{
        price_history,
        robinhood::RobinhoodHistoryClient,
        solana::SolanaHistoryClient,
        HistoryCoverage,
        PriceCoverage,
    },
    ledger::PositionSummary,
    model::Chain,
    providers::gecko::GeckoClient,
    reconstruct::{reconstruct_wallet_position, ReconstructionWarning},
};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
pub struct WalletPositionRequest {
    pub chain: Chain,
    pub token: String,
    pub wallet: String,
    pub launch_timestamp: Option<u64>,
}

impl WalletPositionRequest {
    pub fn validate(&self) -> Result<(), String> {
        validate_chain_address(self.chain, self.token.trim(), "token")?;
        validate_chain_address(self.chain, self.wallet.trim(), "wallet")?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BasisStatus {
    Verified,
    PartialHistory,
    Incomplete,
}

#[derive(Clone, Debug, Serialize)]
pub struct Reconciliation {
    pub observed_current_quantity: Option<Decimal>,
    pub reconstructed_current_quantity: Decimal,
    pub current_balance_matches: Option<bool>,
    pub basis_status: BasisStatus,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct WalletPositionResponse {
    pub chain: Chain,
    pub token: String,
    pub wallet: String,
    pub position: PositionSummary,
    pub behavior: HolderBehaviorMetrics,
    pub history: HistoryCoverage,
    pub pricing: PriceCoverage,
    pub reconciliation: Reconciliation,
    pub reconstruction_warnings: Vec<ReconstructionWarning>,
}

pub async fn analyze_wallet_position(
    http: Client,
    config: &Config,
    gecko: &GeckoClient,
    request: WalletPositionRequest,
) -> Result<WalletPositionResponse, String> {
    request.validate()?;

    let token = request.token.trim().to_string();
    let wallet = request.wallet.trim().to_string();

    let raw = match request.chain {
        Chain::Solana => {
            SolanaHistoryClient::with_fallback(
                http,
                config.solana_rpc_url.clone(),
                config.solana_fallback_rpc_url.clone(),
            )
                .wallet_token_history(&wallet, &token)
                .await
        }
        Chain::Robinhood => {
            RobinhoodHistoryClient::new(http, config.robinhood_rpc_url.clone())
                .wallet_token_history(&wallet, &token)
                .await
        }
        Chain::Bnb => return Err("BNB per-token historical basis is unavailable from the public fallback. Observed execution evidence is available in Wallets; complete basis needs verified wallet-wide history.".into()),
    };

    let (transactions, mut history, pricing) =
        price_history(gecko, request.chain, &token, raw).await;
    let mut reconstructed = reconstruct_wallet_position(&token, transactions);

    let current_balance_matches = history.observed_current_quantity.map(|observed| {
        observed == reconstructed.position.current_quantity
    });

    if current_balance_matches == Some(false) {
        history.complete = false;
        history.notes.push(format!(
            "Reconstructed ending balance ({}) does not match current chain/indexer balance ({}). Missing history is confirmed.",
            reconstructed.position.current_quantity,
            history.observed_current_quantity.unwrap_or_default()
        ));
    }

    // Fee pricing affects all-in entry and realized PnL, not execution-only cost basis.
    // Never publish all-in figures when fee conversion is incomplete.
    if !pricing.fee_pricing_complete {
        reconstructed.position.all_in_average_entry_usd = None;
        reconstructed.position.realized_pnl_usd = None;
        reconstructed.position.realized_pnl_usd_known_portion = None;
    }

    let basis_status = basis_status(
        &history,
        &pricing,
        &reconstructed.position,
        &reconstructed.warnings,
        current_balance_matches,
    );

    let detail = match basis_status {
        BasisStatus::Verified => {
            "History completed, ending balance reconciled, and the remaining position has complete USD basis."
                .to_string()
        }
        BasisStatus::PartialHistory => {
            "Water reconstructed usable basis, but at least one historical-discovery, pricing, or fee-evidence condition is incomplete."
                .to_string()
        }
        BasisStatus::Incomplete => {
            "Water cannot support a complete economic basis for this wallet/token pair from the available evidence."
                .to_string()
        }
    };

    let behavior =
        HolderBehaviorMetrics::from_position(&reconstructed.position, request.launch_timestamp);

    Ok(WalletPositionResponse {
        chain: request.chain,
        token,
        wallet,
        reconciliation: Reconciliation {
            observed_current_quantity: history.observed_current_quantity,
            reconstructed_current_quantity: reconstructed.position.current_quantity,
            current_balance_matches,
            basis_status,
            detail,
        },
        position: reconstructed.position,
        behavior,
        history,
        pricing,
        reconstruction_warnings: reconstructed.warnings,
    })
}

fn basis_status(
    history: &HistoryCoverage,
    pricing: &PriceCoverage,
    position: &PositionSummary,
    warnings: &[ReconstructionWarning],
    current_balance_matches: Option<bool>,
) -> BasisStatus {
    let full_open_basis = position.current_quantity == Decimal::ZERO
        || position.basis_coverage == Decimal::ONE;

    if history.complete
        && current_balance_matches == Some(true)
        && full_open_basis
        && pricing.missing_points == 0
        && pricing.fee_pricing_complete
        && warnings.is_empty()
    {
        BasisStatus::Verified
    } else if current_balance_matches != Some(false)
        && (position.current_quantity == Decimal::ZERO
            || position.known_basis_quantity > Decimal::ZERO)
    {
        BasisStatus::PartialHistory
    } else {
        BasisStatus::Incomplete
    }
}

fn validate_chain_address(chain: Chain, value: &str, field: &str) -> Result<(), String> {
    match chain {
        Chain::Solana => {
            if !(32..=44).contains(&value.len()) {
                return Err(format!("{field} must be a valid Solana base58 address."));
            }
            const BASE58: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
            if !value.chars().all(|character| BASE58.contains(character)) {
                return Err(format!("{field} contains invalid Solana base58 characters."));
            }
        }
        Chain::Robinhood | Chain::Bnb => {
            if value.len() != 42
                || !value.starts_with("0x")
                || !value[2..].chars().all(|character| character.is_ascii_hexdigit())
            {
                return Err(format!("{field} must be a 42-character EVM 0x address."));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryCoverage;

    fn coverage(complete: bool, observed: Option<Decimal>) -> HistoryCoverage {
        HistoryCoverage {
            source: "fixture".to_string(),
            complete,
            pages_read: 1,
            candidate_transactions: 1,
            reconstructed_transactions: 1,
            observed_current_quantity: observed,
            truncated: false,
            notes: vec![],
        }
    }

    #[test]
    fn verified_requires_complete_history_and_reconciliation() {
        let mut ledger = crate::ledger::WalletLedger::default();
        ledger
            .buy(Decimal::from(10), Decimal::from(20), Decimal::ZERO, 1, "buy")
            .unwrap();
        let position = ledger.summary();
        let pricing = PriceCoverage {
            priced_assets: 1,
            requested_assets: 1,
            hourly_points: 1,
            daily_points: 0,
            missing_points: 0,
            fee_points_missing: 0,
            fee_pricing_complete: true,
            notes: vec![],
        };

        assert_eq!(
            basis_status(
                &coverage(true, Some(Decimal::from(10))),
                &pricing,
                &position,
                &[],
                Some(true)
            ),
            BasisStatus::Verified
        );
    }

    #[test]
    fn incomplete_history_never_gets_verified_label() {
        let mut ledger = crate::ledger::WalletLedger::default();
        ledger
            .buy(Decimal::from(10), Decimal::from(20), Decimal::ZERO, 1, "buy")
            .unwrap();
        let position = ledger.summary();
        let pricing = PriceCoverage {
            priced_assets: 1,
            requested_assets: 1,
            hourly_points: 1,
            daily_points: 0,
            missing_points: 0,
            fee_points_missing: 0,
            fee_pricing_complete: true,
            notes: vec![],
        };

        assert_eq!(
            basis_status(
                &coverage(false, Some(Decimal::from(10))),
                &pricing,
                &position,
                &[],
                Some(true)
            ),
            BasisStatus::PartialHistory
        );
    }
}
