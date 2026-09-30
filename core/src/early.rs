use crate::ledger::PositionSummary;
use rust_decimal::Decimal;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct HolderBehaviorMetrics {
    pub first_acquired_at: Option<u64>,
    pub seconds_from_launch: Option<u64>,
    pub current_quantity: Decimal,
    pub peak_quantity: Decimal,
    pub gross_acquired_quantity: Decimal,
    pub retained_from_peak: Decimal,
    pub sold_fraction_of_gross_acquired: Decimal,
    pub transferred_out_fraction_of_gross_acquired: Decimal,
    pub distributed_fraction_of_gross_acquired: Decimal,
    pub basis_coverage: Decimal,
    pub average_entry_usd: Option<Decimal>,
    pub lifetime_average_buy_usd: Option<Decimal>,
    pub net_execution_capital_usd: Option<Decimal>,
    pub break_even_price_usd: Option<Decimal>,
    pub capital_recovered_ratio: Option<Decimal>,
    pub realized_pnl_usd: Option<Decimal>,
}

impl HolderBehaviorMetrics {
    pub fn from_position(position: &PositionSummary, launch_timestamp: Option<u64>) -> Self {
        let gross_acquired = position.bought_quantity
            + position.transferred_in_quantity
            + position.airdropped_quantity;

        let seconds_from_launch = match (position.first_acquired_at, launch_timestamp) {
            (Some(acquired), Some(launch)) if acquired >= launch => Some(acquired - launch),
            _ => None,
        };

        Self {
            first_acquired_at: position.first_acquired_at,
            seconds_from_launch,
            current_quantity: position.current_quantity,
            peak_quantity: position.peak_quantity,
            gross_acquired_quantity: gross_acquired,
            retained_from_peak: ratio(position.current_quantity, position.peak_quantity),
            sold_fraction_of_gross_acquired: ratio(position.sold_quantity, gross_acquired),
            transferred_out_fraction_of_gross_acquired: ratio(
                position.transferred_out_quantity,
                gross_acquired,
            ),
            distributed_fraction_of_gross_acquired: ratio(
                position.sold_quantity + position.transferred_out_quantity,
                gross_acquired,
            ),
            basis_coverage: position.basis_coverage,
            average_entry_usd: position.average_entry_usd,
            lifetime_average_buy_usd: position.lifetime_average_buy_usd,
            net_execution_capital_usd: position.net_execution_capital_usd,
            break_even_price_usd: position.break_even_price_usd,
            capital_recovered_ratio: position.capital_recovered_ratio,
            realized_pnl_usd: position.realized_pnl_usd,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CohortSummary {
    pub wallets: usize,
    pub current_quantity: Decimal,
    pub peak_quantity: Decimal,
    pub gross_acquired_quantity: Decimal,
    pub sold_quantity: Decimal,
    pub transferred_out_quantity: Decimal,
    pub retention_from_peak: Decimal,
    pub distributed_fraction_of_gross_acquired: Decimal,
    pub known_open_basis_quantity: Decimal,
    pub unknown_open_basis_quantity: Decimal,
    pub basis_coverage: Decimal,
    pub open_cost_usd_known: Decimal,
    pub weighted_average_entry_usd: Option<Decimal>,
}

pub fn summarize_cohort(positions: &[PositionSummary]) -> CohortSummary {
    let mut current_quantity = Decimal::ZERO;
    let mut peak_quantity = Decimal::ZERO;
    let mut gross_acquired_quantity = Decimal::ZERO;
    let mut sold_quantity = Decimal::ZERO;
    let mut transferred_out_quantity = Decimal::ZERO;
    let mut known_open_basis_quantity = Decimal::ZERO;
    let mut unknown_open_basis_quantity = Decimal::ZERO;
    let mut open_cost_usd_known = Decimal::ZERO;

    for position in positions {
        current_quantity += position.current_quantity;
        peak_quantity += position.peak_quantity;
        gross_acquired_quantity += position.bought_quantity
            + position.transferred_in_quantity
            + position.airdropped_quantity;
        sold_quantity += position.sold_quantity;
        transferred_out_quantity += position.transferred_out_quantity;
        known_open_basis_quantity += position.known_basis_quantity;
        unknown_open_basis_quantity += position.unknown_basis_quantity;
        open_cost_usd_known += position.open_cost_usd_known;
    }

    let total_open_basis_quantity = known_open_basis_quantity + unknown_open_basis_quantity;
    let weighted_average_entry_usd = if unknown_open_basis_quantity == Decimal::ZERO {
        nonzero_div(open_cost_usd_known, current_quantity)
    } else {
        None
    };

    CohortSummary {
        wallets: positions.len(),
        current_quantity,
        peak_quantity,
        gross_acquired_quantity,
        sold_quantity,
        transferred_out_quantity,
        retention_from_peak: ratio(current_quantity, peak_quantity),
        distributed_fraction_of_gross_acquired: ratio(
            sold_quantity + transferred_out_quantity,
            gross_acquired_quantity,
        ),
        known_open_basis_quantity,
        unknown_open_basis_quantity,
        basis_coverage: ratio(known_open_basis_quantity, total_open_basis_quantity),
        open_cost_usd_known,
        weighted_average_entry_usd,
    }
}

fn ratio(numerator: Decimal, denominator: Decimal) -> Decimal {
    if denominator <= Decimal::ZERO {
        Decimal::ZERO
    } else {
        numerator / denominator
    }
}

fn nonzero_div(numerator: Decimal, denominator: Decimal) -> Option<Decimal> {
    if denominator > Decimal::ZERO {
        Some(numerator / denominator)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::WalletLedger;

    fn d(value: i64) -> Decimal {
        Decimal::from(value)
    }

    #[test]
    fn reports_distribution_without_subjective_labels() {
        let mut ledger = WalletLedger::default();
        ledger.buy(d(100), d(100), Decimal::ZERO, 110, "buy").unwrap();
        ledger.sell(d(25), d(50), Decimal::ZERO, 200).unwrap();
        ledger.transfer_out(d(25), 210).unwrap();

        let metrics = HolderBehaviorMetrics::from_position(&ledger.summary(), Some(100));

        assert_eq!(metrics.seconds_from_launch, Some(10));
        assert_eq!(metrics.retained_from_peak, Decimal::new(5, 1));
        assert_eq!(
            metrics.distributed_fraction_of_gross_acquired,
            Decimal::new(5, 1)
        );
    }

    #[test]
    fn cohort_average_is_quantity_weighted_and_requires_complete_basis() {
        let mut a = WalletLedger::default();
        a.buy(d(100), d(100), Decimal::ZERO, 10, "a").unwrap();

        let mut b = WalletLedger::default();
        b.buy(d(300), d(900), Decimal::ZERO, 10, "b").unwrap();

        let cohort = summarize_cohort(&[a.summary(), b.summary()]);

        assert_eq!(cohort.current_quantity, d(400));
        assert_eq!(cohort.weighted_average_entry_usd, Some(Decimal::new(25, 1)));
        assert_eq!(cohort.basis_coverage, Decimal::ONE);
    }
}
