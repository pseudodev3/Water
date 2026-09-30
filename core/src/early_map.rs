use crate::{
    cohort::holder_candidates,
    config::Config,
    history::HolderCandidate,
    model::Chain,
    position::{analyze_wallet_position, BasisStatus, WalletPositionRequest, WalletPositionResponse},
    providers::gecko::GeckoClient,
};
use futures::{stream, StreamExt};
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

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MovementHistoryStatus {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Clone, Debug, Serialize)]
pub struct EarlyHolderView {
    pub rank: usize,
    pub wallet: String,
    /// Current quantity always comes from the holder candidate source rather than
    /// the reconstructed wallet ledger, so a missing history never hides the holder.
    pub current_quantity: f64,
    pub first_acquired_at: Option<u64>,
    pub peak_quantity: Option<f64>,
    pub retained_from_peak: Option<f64>,
    pub distributed_fraction: Option<f64>,
    pub basis_coverage: Option<f64>,
    pub average_entry_usd: Option<f64>,
    pub current_price_usd: Option<f64>,
    pub current_multiple_on_entry: Option<f64>,
    pub basis_status: Option<BasisStatus>,
    pub movement_history: MovementHistoryStatus,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct EarlyHolderMapResponse {
    pub chain: Chain,
    pub token: String,
    pub observed_at_unix: u64,
    pub wallets_requested: usize,
    pub wallets_listed: usize,
    pub wallets_reconstructed: usize,
    pub complete_movement_histories: usize,
    pub cohort_retained_from_peak: Option<f64>,
    pub cohort_distributed_fraction: Option<f64>,
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
    let candidate_future =
        holder_candidates(http.clone(), config, request.chain, &token, limit);

    let (market_result, candidate_result) = tokio::join!(market_future, candidate_future);
    let (candidates, _candidate_source, mut notes) = candidate_result?;
    let current_price_usd = market_result.ok().and_then(|market| market.price_usd);

    // Each wallet history is independent, but cold historical pricing can make
    // several Gecko requests. Keep concurrency at two so one slow wallet does not
    // block the map without bursting the public market-data budget.
    let analyses = stream::iter(candidates.into_iter().map(|candidate| {
        let http = http.clone();
        let token = token.clone();
        async move {
            let result = analyze_candidate(
                http,
                config,
                gecko,
                request.chain,
                token,
                candidate.clone(),
            )
            .await;
            (candidate, result)
        }
    }))
    .buffer_unordered(2)
    .collect::<Vec<_>>()
    .await;

    let mut holders = Vec::with_capacity(analyses.len());
    let mut reconstructed = 0usize;
    let mut complete_movement_histories = 0usize;
    let mut complete_positions = Vec::new();

    for (candidate, result) in analyses {
        match result {
            Ok(analysis) => {
                reconstructed += 1;

                let movement_complete = analysis.history.complete
                    && analysis.reconciliation.current_balance_matches == Some(true);

                if movement_complete {
                    complete_movement_histories += 1;
                    complete_positions.push(analysis.position.clone());
                }

                holders.push(view_from_analysis(
                    candidate,
                    analysis,
                    movement_complete,
                    current_price_usd,
                ));
            }
            Err(error) => {
                notes.push(format!(
                    "Could not reconstruct {} history: {error}",
                    candidate.wallet
                ));
                holders.push(view_without_history(candidate, current_price_usd, error));
            }
        }
    }

    // Do not publish a cohort-wide retention/distribution number unless every
    // displayed holder has a complete, balance-reconciled movement history.
    // A partial cohort would systematically understate earlier peaks/distribution.
    let (cohort_retained_from_peak, cohort_distributed_fraction) =
        if !holders.is_empty() && complete_movement_histories == holders.len() {
            summarize_complete_positions(&complete_positions)
        } else {
            (None, None)
        };

    // Preserve the candidate ranking. "Early" is a view over current large
    // wallets; first-acquisition timestamps are evidence attached to each row,
    // not a reason to silently reorder or drop a top holder.
    holders.sort_by_key(|holder| holder.rank);

    Ok(EarlyHolderMapResponse {
        chain: request.chain,
        token,
        observed_at_unix: now_unix(),
        wallets_requested: limit,
        wallets_listed: holders.len(),
        wallets_reconstructed: reconstructed,
        complete_movement_histories,
        cohort_retained_from_peak,
        cohort_distributed_fraction,
        holders,
        notes,
    })
}

async fn analyze_candidate(
    http: Client,
    config: &Config,
    gecko: &GeckoClient,
    chain: Chain,
    token: String,
    candidate: HolderCandidate,
) -> Result<WalletPositionResponse, String> {
    analyze_wallet_position(
        http,
        config,
        gecko,
        WalletPositionRequest {
            chain,
            token,
            wallet: candidate.wallet,
            launch_timestamp: None,
        },
    )
    .await
}

fn view_from_analysis(
    candidate: HolderCandidate,
    analysis: WalletPositionResponse,
    movement_complete: bool,
    current_price_usd: Option<f64>,
) -> EarlyHolderView {
    let behavior = &analysis.behavior;
    let average_entry_usd = decimal_option_to_f64(behavior.average_entry_usd);
    let current_multiple_on_entry = match (current_price_usd, average_entry_usd) {
        (Some(price), Some(entry)) if price.is_finite() && entry > 0.0 => Some(price / entry),
        _ => None,
    };

    let movement_history = if movement_complete {
        MovementHistoryStatus::Complete
    } else {
        MovementHistoryStatus::Partial
    };

    let detail = holder_detail(&analysis, movement_complete);

    EarlyHolderView {
        rank: candidate.source_rank,
        wallet: analysis.wallet,
        current_quantity: decimal_to_f64(candidate.current_quantity),
        first_acquired_at: behavior.first_acquired_at,
        peak_quantity: movement_complete.then(|| decimal_to_f64(behavior.peak_quantity)),
        retained_from_peak: movement_complete.then(|| {
            decimal_to_f64(behavior.retained_from_peak).clamp(0.0, 1.0)
        }),
        distributed_fraction: movement_complete.then(|| {
            decimal_to_f64(behavior.distributed_fraction_of_gross_acquired).clamp(0.0, 1.0)
        }),
        basis_coverage: Some(decimal_to_f64(behavior.basis_coverage).clamp(0.0, 1.0)),
        average_entry_usd,
        current_price_usd,
        current_multiple_on_entry,
        basis_status: Some(analysis.reconciliation.basis_status),
        movement_history,
        detail,
    }
}

fn view_without_history(
    candidate: HolderCandidate,
    current_price_usd: Option<f64>,
    error: String,
) -> EarlyHolderView {
    EarlyHolderView {
        rank: candidate.source_rank,
        wallet: candidate.wallet,
        current_quantity: decimal_to_f64(candidate.current_quantity),
        first_acquired_at: None,
        peak_quantity: None,
        retained_from_peak: None,
        distributed_fraction: None,
        basis_coverage: None,
        average_entry_usd: None,
        current_price_usd,
        current_multiple_on_entry: None,
        basis_status: None,
        movement_history: MovementHistoryStatus::Unavailable,
        detail: format!(
            "Current holder balance is verified, but Water could not reconstruct this wallet's movement history: {error}"
        ),
    }
}

fn holder_detail(analysis: &WalletPositionResponse, movement_complete: bool) -> String {
    if !movement_complete {
        return "Current balance is known, but historical discovery did not fully reconcile; peak and distribution metrics are withheld.".to_string();
    }

    match analysis.reconciliation.basis_status {
        BasisStatus::Verified => {
            "Movement history reconciles and the remaining position has verified USD entry basis."
                .to_string()
        }
        BasisStatus::PartialHistory => {
            "Movement history reconciles, but only part of the remaining position has supported USD entry basis."
                .to_string()
        }
        BasisStatus::Incomplete => {
            "Movement history reconciles, but Water cannot prove what the remaining tokens economically cost (for example, a transfer-in can have unknown basis)."
                .to_string()
        }
    }
}

fn summarize_complete_positions(
    positions: &[crate::ledger::PositionSummary],
) -> (Option<f64>, Option<f64>) {
    if positions.is_empty() {
        return (None, None);
    }

    let mut current = Decimal::ZERO;
    let mut peak = Decimal::ZERO;
    let mut acquired = Decimal::ZERO;
    let mut distributed = Decimal::ZERO;

    for position in positions {
        current += position.current_quantity;
        peak += position.peak_quantity;

        let gross_acquired =
            position.bought_quantity + position.transferred_in_quantity + position.airdropped_quantity;
        acquired += gross_acquired;
        distributed += position.sold_quantity + position.transferred_out_quantity;
    }

    let retention = ratio_to_f64(current, peak);
    let distribution = ratio_to_f64(distributed, acquired);
    (retention, distribution)
}

fn ratio_to_f64(numerator: Decimal, denominator: Decimal) -> Option<f64> {
    if denominator <= Decimal::ZERO {
        return None;
    }
    Some(decimal_to_f64(numerator / denominator).clamp(0.0, 1.0))
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

    #[test]
    fn ratio_with_zero_denominator_stays_unknown() {
        assert_eq!(ratio_to_f64(Decimal::ONE, Decimal::ZERO), None);
    }
}
