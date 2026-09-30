use crate::{
    flow::{EconomicEvent, WalletTransactionFlow},
    ledger::{LedgerError, PositionSummary, WalletLedger},
};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct ReconstructionWarning {
    pub tx_id: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReconstructionReport {
    pub position: PositionSummary,
    pub transactions_seen: usize,
    pub economic_events_seen: usize,
    pub warnings: Vec<ReconstructionWarning>,
}

pub fn reconstruct_wallet_position(
    target_asset: &str,
    mut transactions: Vec<WalletTransactionFlow>,
) -> ReconstructionReport {
    transactions.sort_by_key(|tx| tx.timestamp);

    let mut ledger = WalletLedger::default();
    let mut warnings = Vec::new();
    let mut economic_events_seen = 0usize;

    for tx in &transactions {
        let event = crate::flow::classify_target_flow(tx, target_asset);

        let result = match event {
            EconomicEvent::Buy {
                quantity,
                cost_usd: Some(cost_usd),
                network_fee_usd,
                ..
            } => {
                economic_events_seen += 1;
                ledger.buy(
                    quantity,
                    cost_usd,
                    network_fee_usd,
                    tx.timestamp,
                    tx.tx_id.clone(),
                )
            }
            EconomicEvent::Buy {
                quantity,
                cost_usd: None,
                network_fee_usd,
                ..
            } => {
                economic_events_seen += 1;
                ledger.buy_unknown_cost(
                    quantity,
                    network_fee_usd,
                    tx.timestamp,
                    tx.tx_id.clone(),
                )
            }
            EconomicEvent::Sell {
                quantity,
                proceeds_usd: Some(proceeds_usd),
                network_fee_usd,
                ..
            } => {
                economic_events_seen += 1;
                ledger
                    .sell(quantity, proceeds_usd, network_fee_usd, tx.timestamp)
                    .map(|_| ())
            }
            EconomicEvent::Sell {
                quantity,
                proceeds_usd: None,
                network_fee_usd,
                ..
            } => {
                economic_events_seen += 1;
                ledger
                    .sell_unknown_proceeds(quantity, network_fee_usd, tx.timestamp)
                    .map(|_| ())
            }
            EconomicEvent::TransferIn { quantity } => {
                economic_events_seen += 1;
                ledger.reconcile_unknown_in(quantity, tx.timestamp, tx.tx_id.clone())
            }
            EconomicEvent::TransferOut { quantity } => {
                economic_events_seen += 1;
                ledger.transfer_out(quantity, tx.timestamp).map(|_| ())
            }
            EconomicEvent::NoTargetChange => Ok(()),
        };

        if let Err(error) = result {
            warnings.push(ReconstructionWarning {
                tx_id: tx.tx_id.clone(),
                detail: ledger_error_detail(error),
            });
        }
    }

    ReconstructionReport {
        position: ledger.summary(),
        transactions_seen: transactions.len(),
        economic_events_seen,
        warnings,
    }
}

fn ledger_error_detail(error: LedgerError) -> String {
    match error {
        LedgerError::InsufficientBalance {
            requested,
            available,
        } => format!(
            "Historical reconstruction became incomplete before this disposal: requested {requested}, reconstructed balance {available}. The event was not force-applied."
        ),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::AssetFlow;
    use rust_decimal::Decimal;

    fn d(value: i64) -> Decimal {
        Decimal::from(value)
    }

    #[test]
    fn chronological_reconstruction_uses_exact_asset_deltas() {
        let transactions = vec![
            WalletTransactionFlow {
                tx_id: "sell".to_string(),
                timestamp: 30,
                network_fee_usd: Decimal::ZERO,
                assets: vec![
                    AssetFlow {
                        asset_id: "TOKEN".to_string(),
                        delta: d(-40),
                        usd_price: None,
                    },
                    AssetFlow {
                        asset_id: "USDC".to_string(),
                        delta: d(120),
                        usd_price: Some(Decimal::ONE),
                    },
                ],
            },
            WalletTransactionFlow {
                tx_id: "buy".to_string(),
                timestamp: 10,
                network_fee_usd: Decimal::ZERO,
                assets: vec![
                    AssetFlow {
                        asset_id: "TOKEN".to_string(),
                        delta: d(100),
                        usd_price: None,
                    },
                    AssetFlow {
                        asset_id: "USDC".to_string(),
                        delta: d(-100),
                        usd_price: Some(Decimal::ONE),
                    },
                ],
            },
        ];

        let report = reconstruct_wallet_position("TOKEN", transactions);

        assert_eq!(report.position.current_quantity, d(60));
        assert_eq!(report.position.average_entry_usd, Some(Decimal::ONE));
        assert_eq!(report.position.realized_pnl_usd, Some(d(80)));
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn missing_earlier_history_surfaces_warning_instead_of_faking_basis() {
        let transactions = vec![WalletTransactionFlow {
            tx_id: "sell-first".to_string(),
            timestamp: 10,
            network_fee_usd: Decimal::ZERO,
            assets: vec![
                AssetFlow {
                    asset_id: "TOKEN".to_string(),
                    delta: d(-10),
                    usd_price: None,
                },
                AssetFlow {
                    asset_id: "USDC".to_string(),
                    delta: d(20),
                    usd_price: Some(Decimal::ONE),
                },
            ],
        }];

        let report = reconstruct_wallet_position("TOKEN", transactions);

        assert_eq!(report.position.current_quantity, Decimal::ZERO);
        assert_eq!(report.warnings.len(), 1);
    }
}
