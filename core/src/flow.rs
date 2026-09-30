use rust_decimal::Decimal;
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct AssetFlow {
    pub asset_id: String,
    /// Positive means the wallet received the asset; negative means it spent/sent it.
    pub delta: Decimal,
    /// USD price of one unit at the transaction timestamp.
    pub usd_price: Option<Decimal>,
}

#[derive(Clone, Debug)]
pub struct WalletTransactionFlow {
    pub tx_id: String,
    pub timestamp: u64,
    /// Network fee only. Adapters must remove fee/rent effects from asset deltas.
    pub network_fee_usd: Decimal,
    pub assets: Vec<AssetFlow>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationConfidence {
    High,
    Medium,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EconomicEvent {
    Buy {
        quantity: Decimal,
        quote_asset: Option<String>,
        quote_quantity: Option<Decimal>,
        cost_usd: Option<Decimal>,
        network_fee_usd: Decimal,
        confidence: ClassificationConfidence,
    },
    Sell {
        quantity: Decimal,
        quote_asset: Option<String>,
        quote_quantity: Option<Decimal>,
        proceeds_usd: Option<Decimal>,
        network_fee_usd: Decimal,
        confidence: ClassificationConfidence,
    },
    TransferIn {
        quantity: Decimal,
    },
    TransferOut {
        quantity: Decimal,
    },
    NoTargetChange,
}

pub fn classify_target_flow(
    tx: &WalletTransactionFlow,
    target_asset: &str,
) -> EconomicEvent {
    let target_delta = tx
        .assets
        .iter()
        .filter(|flow| same_asset(&flow.asset_id, target_asset))
        .fold(Decimal::ZERO, |sum, flow| sum + flow.delta);

    if target_delta == Decimal::ZERO {
        return EconomicEvent::NoTargetChange;
    }

    let opposite: Vec<&AssetFlow> = tx
        .assets
        .iter()
        .filter(|flow| !same_asset(&flow.asset_id, target_asset))
        .filter(|flow| {
            if target_delta > Decimal::ZERO {
                flow.delta < Decimal::ZERO
            } else {
                flow.delta > Decimal::ZERO
            }
        })
        .collect();

    if opposite.is_empty() {
        return if target_delta > Decimal::ZERO {
            EconomicEvent::TransferIn {
                quantity: target_delta,
            }
        } else {
            EconomicEvent::TransferOut {
                quantity: -target_delta,
            }
        };
    }

    let confidence = if opposite.len() == 1 {
        ClassificationConfidence::High
    } else {
        ClassificationConfidence::Medium
    };

    let best_priced = opposite
        .iter()
        .filter_map(|flow| {
            let price = flow.usd_price?;
            if price <= Decimal::ZERO {
                return None;
            }
            let usd_value = abs(flow.delta) * price;
            Some((*flow, usd_value))
        })
        .max_by(|(_, left), (_, right)| left.cmp(right));

    let fallback = opposite.first().copied();

    if target_delta > Decimal::ZERO {
        match best_priced {
            Some((quote, usd_value)) => EconomicEvent::Buy {
                quantity: target_delta,
                quote_asset: Some(quote.asset_id.clone()),
                quote_quantity: Some(abs(quote.delta)),
                cost_usd: Some(usd_value),
                network_fee_usd: tx.network_fee_usd,
                confidence,
            },
            None => EconomicEvent::Buy {
                quantity: target_delta,
                quote_asset: fallback.map(|flow| flow.asset_id.clone()),
                quote_quantity: fallback.map(|flow| abs(flow.delta)),
                cost_usd: None,
                network_fee_usd: tx.network_fee_usd,
                confidence,
            },
        }
    } else {
        match best_priced {
            Some((quote, usd_value)) => EconomicEvent::Sell {
                quantity: -target_delta,
                quote_asset: Some(quote.asset_id.clone()),
                quote_quantity: Some(abs(quote.delta)),
                proceeds_usd: Some(usd_value),
                network_fee_usd: tx.network_fee_usd,
                confidence,
            },
            None => EconomicEvent::Sell {
                quantity: -target_delta,
                quote_asset: fallback.map(|flow| flow.asset_id.clone()),
                quote_quantity: fallback.map(|flow| abs(flow.delta)),
                proceeds_usd: None,
                network_fee_usd: tx.network_fee_usd,
                confidence,
            },
        }
    }
}

fn same_asset(left: &str, right: &str) -> bool {
    if left.starts_with("0x") && right.starts_with("0x") {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn abs(value: Decimal) -> Decimal {
    if value < Decimal::ZERO {
        -value
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(value: i64) -> Decimal {
        Decimal::from(value)
    }

    #[test]
    fn exact_quote_delta_drives_buy_cost() {
        let tx = WalletTransactionFlow {
            tx_id: "buy".to_string(),
            timestamp: 1,
            network_fee_usd: Decimal::new(2, 1),
            assets: vec![
                AssetFlow {
                    asset_id: "TOKEN".to_string(),
                    delta: d(2_000),
                    usd_price: None,
                },
                AssetFlow {
                    asset_id: "SOL".to_string(),
                    delta: Decimal::new(-84, 2),
                    usd_price: Some(d(200)),
                },
            ],
        };

        match classify_target_flow(&tx, "TOKEN") {
            EconomicEvent::Buy {
                quantity,
                cost_usd,
                quote_quantity,
                ..
            } => {
                assert_eq!(quantity, d(2_000));
                assert_eq!(quote_quantity, Some(Decimal::new(84, 2)));
                assert_eq!(cost_usd, Some(d(168)));
            }
            other => panic!("expected buy, got {other:?}"),
        }
    }

    #[test]
    fn unpriced_quote_stays_a_trade_with_unknown_basis() {
        let tx = WalletTransactionFlow {
            tx_id: "buy".to_string(),
            timestamp: 1,
            network_fee_usd: Decimal::ZERO,
            assets: vec![
                AssetFlow {
                    asset_id: "TOKEN".to_string(),
                    delta: d(100),
                    usd_price: None,
                },
                AssetFlow {
                    asset_id: "UNKNOWN_QUOTE".to_string(),
                    delta: d(-5),
                    usd_price: None,
                },
            ],
        };

        match classify_target_flow(&tx, "TOKEN") {
            EconomicEvent::Buy { cost_usd, .. } => assert_eq!(cost_usd, None),
            other => panic!("expected buy, got {other:?}"),
        }
    }

    #[test]
    fn no_opposite_asset_is_a_transfer_not_a_fake_trade() {
        let tx = WalletTransactionFlow {
            tx_id: "transfer".to_string(),
            timestamp: 1,
            network_fee_usd: Decimal::ZERO,
            assets: vec![AssetFlow {
                asset_id: "TOKEN".to_string(),
                delta: d(100),
                usd_price: None,
            }],
        };

        match classify_target_flow(&tx, "TOKEN") {
            EconomicEvent::TransferIn { quantity } => assert_eq!(quantity, d(100)),
            other => panic!("expected transfer in, got {other:?}"),
        }
    }

    #[test]
    fn largest_priced_opposite_flow_wins_for_batched_transactions() {
        let tx = WalletTransactionFlow {
            tx_id: "batch".to_string(),
            timestamp: 1,
            network_fee_usd: Decimal::ZERO,
            assets: vec![
                AssetFlow {
                    asset_id: "TOKEN".to_string(),
                    delta: d(100),
                    usd_price: None,
                },
                AssetFlow {
                    asset_id: "USDC".to_string(),
                    delta: d(-200),
                    usd_price: Some(Decimal::ONE),
                },
                AssetFlow {
                    asset_id: "DUST".to_string(),
                    delta: d(-1),
                    usd_price: Some(Decimal::ONE),
                },
            ],
        };

        match classify_target_flow(&tx, "TOKEN") {
            EconomicEvent::Buy {
                quote_asset,
                cost_usd,
                confidence,
                ..
            } => {
                assert_eq!(quote_asset.as_deref(), Some("USDC"));
                assert_eq!(cost_usd, Some(d(200)));
                assert_eq!(confidence, ClassificationConfidence::Medium);
            }
            other => panic!("expected buy, got {other:?}"),
        }
    }
}
