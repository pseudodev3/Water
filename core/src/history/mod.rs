pub mod robinhood;
pub mod solana;

use crate::{
    flow::{AssetFlow, WalletTransactionFlow},
    model::Chain,
    providers::gecko::{GeckoClient, PriceGranularity},
};
use rust_decimal::Decimal;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct RawAssetFlow {
    pub asset_id: String,
    pub delta: Decimal,
}

#[derive(Clone, Debug)]
pub struct RawWalletTransaction {
    pub tx_id: String,
    pub timestamp: u64,
    pub network_fee_asset_id: Option<String>,
    pub network_fee_quantity: Option<Decimal>,
    pub assets: Vec<RawAssetFlow>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HistoryCoverage {
    pub source: String,
    pub complete: bool,
    pub pages_read: usize,
    pub candidate_transactions: usize,
    pub reconstructed_transactions: usize,
    pub truncated: bool,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct RawHistory {
    pub transactions: Vec<RawWalletTransaction>,
    pub coverage: HistoryCoverage,
}

#[derive(Clone, Debug, Serialize)]
pub struct PriceCoverage {
    pub priced_assets: usize,
    pub requested_assets: usize,
    pub hourly_points: usize,
    pub daily_points: usize,
    pub missing_points: usize,
    pub fee_points_missing: usize,
    pub fee_pricing_complete: bool,
    pub notes: Vec<String>,
}

pub async fn price_history(
    gecko: &GeckoClient,
    chain: Chain,
    target_asset: &str,
    raw: RawHistory,
) -> (Vec<WalletTransactionFlow>, HistoryCoverage, PriceCoverage) {
    let mut timestamps_by_asset: HashMap<String, Vec<u64>> = HashMap::new();

    for tx in &raw.transactions {
        for flow in &tx.assets {
            if !same_asset(&flow.asset_id, target_asset) && flow.delta != Decimal::ZERO {
                timestamps_by_asset
                    .entry(flow.asset_id.clone())
                    .or_default()
                    .push(tx.timestamp);
            }
        }

        if let Some(fee_asset) = &tx.network_fee_asset_id {
            timestamps_by_asset
                .entry(fee_asset.clone())
                .or_default()
                .push(tx.timestamp);
        }
    }

    // Price the most frequently used quote assets first. This bounds GeckoTerminal
    // usage under the public API limit while preserving the raw onchain history.
    let mut ranked_assets: Vec<(String, Vec<u64>)> = timestamps_by_asset.into_iter().collect();
    ranked_assets.sort_by(|left, right| right.1.len().cmp(&left.1.len()));
    let requested_assets = ranked_assets.len();
    ranked_assets.truncate(4);

    let mut prices: HashMap<(String, u64), (Decimal, PriceGranularity)> = HashMap::new();
    let mut priced_assets = 0usize;
    let mut hourly_points = 0usize;
    let mut daily_points = 0usize;
    let mut notes = Vec::new();

    for (asset, mut timestamps) in ranked_assets {
        timestamps.sort_unstable();
        timestamps.dedup();

        match gecko.historical_usd_prices(chain, &asset, &timestamps).await {
            Ok(asset_prices) => {
                if !asset_prices.is_empty() {
                    priced_assets += 1;
                }

                for (timestamp, point) in asset_prices {
                    match point.granularity {
                        PriceGranularity::Hour => hourly_points += 1,
                        PriceGranularity::Day => daily_points += 1,
                    }
                    prices.insert((normalized_asset(&asset), timestamp), (point.usd_price, point.granularity));
                }
            }
            Err(error) => {
                notes.push(format!("Could not price {asset}: {error}"));
            }
        }
    }

    if requested_assets > 4 {
        notes.push(format!(
            "{} low-frequency quote assets were left unpriced to stay within the public market-data budget.",
            requested_assets - 4
        ));
    }

    let mut missing_points = 0usize;
    let mut fee_points_missing = 0usize;
    let mut seen_missing: HashSet<(String, u64)> = HashSet::new();
    let mut transactions = Vec::with_capacity(raw.transactions.len());

    for tx in raw.transactions {
        let mut assets = Vec::with_capacity(tx.assets.len());

        for flow in tx.assets {
            let key = (normalized_asset(&flow.asset_id), tx.timestamp);
            let usd_price = prices.get(&key).map(|(price, _)| *price);

            if !same_asset(&flow.asset_id, target_asset)
                && flow.delta != Decimal::ZERO
                && usd_price.is_none()
                && seen_missing.insert(key)
            {
                missing_points += 1;
            }

            assets.push(AssetFlow {
                asset_id: flow.asset_id,
                delta: flow.delta,
                usd_price,
            });
        }

        let network_fee_usd = match (
            tx.network_fee_asset_id.as_ref(),
            tx.network_fee_quantity,
        ) {
            (Some(asset), Some(quantity)) => {
                match prices.get(&(normalized_asset(asset), tx.timestamp)) {
                    Some((price, _)) => quantity * *price,
                    None => {
                        fee_points_missing += 1;
                        Decimal::ZERO
                    }
                }
            }
            _ => Decimal::ZERO,
        };

        transactions.push(WalletTransactionFlow {
            tx_id: tx.tx_id,
            timestamp: tx.timestamp,
            network_fee_usd,
            assets,
        });
    }

    if missing_points > 0 {
        notes.push(
            "Missing quote-price points leave execution USD basis unknown; Water keeps the underlying asset deltas intact."
                .to_string(),
        );
    }

    let price_coverage = PriceCoverage {
        priced_assets,
        requested_assets,
        hourly_points,
        daily_points,
        missing_points,
        fee_points_missing,
        fee_pricing_complete: fee_points_missing == 0,
        notes,
    };

    (transactions, raw.coverage, price_coverage)
}

fn same_asset(left: &str, right: &str) -> bool {
    if left.starts_with("0x") && right.starts_with("0x") {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn normalized_asset(asset: &str) -> String {
    if asset.starts_with("0x") {
        asset.to_ascii_lowercase()
    } else {
        asset.to_string()
    }
}
