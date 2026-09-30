use crate::{
    cohort::{analyze_holder_cohort, HolderCohortRequest},
    config::Config,
    position::BasisStatus,
    providers::gecko::GeckoClient,
    model::Chain,
};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_LIMIT: usize = 3;
const MAX_LIMIT: usize = 3;

#[derive(Clone, Debug, Deserialize)]
pub struct EarlyHolderMapRequest {
    pub chain: Chain,
    pub token: String,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EarlyHolderView {
    pub rank: usize,
    pub wallet: String,
    pub first_acquired_at: Option<u64>,
    pub current_quantity: f64,
    pub peak_quantity: f64,
    pub retained_from_peak: f64,
    pub distributed_fraction: f64,
    pub basis_coverage: f64,
    pub average_entry_usd: Option<f64>,
    pub current_price_usd: Option<f64>,
    pub current_multiple_on_entry: Option<f64>,
    pub basis_status: BasisStatus,
}

#[derive(Clone, Debug, Serialize)]
pub struct EarlyHolderMapResponse {
    pub chain: Chain,
    pub token: String,
    pub observed_at_unix: u64,
    pub wallets_requested: usize,
    pub wallets_reconstructed: usize,
    pub cohort_retained_from_peak: f64,
    pub cohort_distributed_fraction: f64,
    pub holders: Vec<EarlyHolderView>,
    pub notes: Vec<String>,
}

pub async fn analyze_early_holder_map(
    http: Client,
    config: &Config,
    gecko: &GeckoClient,
    request: EarlyHolderMapRequest,
) -> Result<EarlyHolderMapResponse, String> {
    let limit = request.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let token = request.token.trim().to_string();

    let market_future = gecko.market_snapshot(request.chain, &token);
    let cohort_future = analyze_holder_cohort(
        http,
        config,
        gecko,
        HolderCohortRequest {
            chain: request.chain,
            token: token.clone(),
            limit: Some(limit),
            launch_timestamp: None,
        },
    );

    let (market_result, cohort_result) = tokio::join!(market_future, cohort_future);
    let cohort = cohort_result?;
    let current_price_usd = market_result.ok().and_then(|market| market.price_usd);

    let holders = cohort
        .members
        .iter()
        .map(|member| {
            let behavior = &member.analysis.behavior;
            let average_entry_usd = decimal_option_to_f64(behavior.average_entry_usd);
            let current_multiple_on_entry = match (current_price_usd, average_entry_usd) {
                (Some(price), Some(entry)) if price.is_finite() && entry > 0.0 => {
                    Some(price / entry)
                }
                _ => None,
            };

            EarlyHolderView {
                rank: member.source_rank,
                wallet: member.analysis.wallet.clone(),
                first_acquired_at: behavior.first_acquired_at,
                current_quantity: decimal_to_f64(behavior.current_quantity),
                peak_quantity: decimal_to_f64(behavior.peak_quantity),
                retained_from_peak: decimal_to_f64(behavior.retained_from_peak)
                    .clamp(0.0, 1.0),
                distributed_fraction: decimal_to_f64(
                    behavior.distributed_fraction_of_gross_acquired,
                )
                .clamp(0.0, 1.0),
                basis_coverage: decimal_to_f64(behavior.basis_coverage)
                    .clamp(0.0, 1.0),
                average_entry_usd,
                current_price_usd,
                current_multiple_on_entry,
                basis_status: member.analysis.reconciliation.basis_status,
            }
        })
        .collect::<Vec<_>>();

    Ok(EarlyHolderMapResponse {
        chain: request.chain,
        token,
        observed_at_unix: now_unix(),
        wallets_requested: cohort.members_requested,
        wallets_reconstructed: holders.len(),
        cohort_retained_from_peak: decimal_to_f64(cohort.cohort.retention_from_peak)
            .clamp(0.0, 1.0),
        cohort_distributed_fraction: decimal_to_f64(
            cohort.cohort.distributed_fraction_of_gross_acquired,
        )
        .clamp(0.0, 1.0),
        holders,
        notes: cohort.notes,
    })
}

fn decimal_to_f64(value: Decimal) -> f64 {
    value.to_string().parse::<f64>().unwrap_or(0.0)
}

fn decimal_option_to_f64(value: Option<Decimal>) -> Option<f64> {
    value.and_then(|value| value.to_string().parse::<f64>().ok())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_conversion_is_lossy_but_stable_for_ui_values() {
        assert_eq!(decimal_to_f64(Decimal::new(218, 3)), 0.218);
        assert_eq!(decimal_option_to_f64(Some(Decimal::from(2))), Some(2.0));
    }
}
