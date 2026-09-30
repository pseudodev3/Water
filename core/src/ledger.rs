use rust_decimal::Decimal;
use serde::Serialize;
use std::collections::VecDeque;
use thiserror::Error;

#[derive(Clone, Debug)]
struct Lot {
    quantity: Decimal,
    cost_usd: Option<Decimal>,
    acquisition_fee_usd: Decimal,
    acquired_at: u64,
    origin_tx_id: String,
}

#[derive(Clone, Debug)]
pub struct CarriedLot {
    pub quantity: Decimal,
    pub cost_usd: Option<Decimal>,
    pub acquisition_fee_usd: Decimal,
    pub acquired_at: u64,
    pub origin_tx_id: String,
}

#[derive(Clone, Debug, Default)]
pub struct BasisPacket {
    pub lots: Vec<CarriedLot>,
}

impl BasisPacket {
    pub fn quantity(&self) -> Decimal {
        self.lots
            .iter()
            .fold(Decimal::ZERO, |total, lot| total + lot.quantity)
    }

    pub fn known_cost_usd(&self) -> Decimal {
        self.lots.iter().fold(Decimal::ZERO, |total, lot| {
            total + lot.cost_usd.unwrap_or(Decimal::ZERO)
        })
    }
}

#[derive(Clone, Debug)]
pub struct Disposal {
    pub quantity: Decimal,
    pub known_basis_quantity: Decimal,
    pub unknown_basis_quantity: Decimal,
    pub known_cost_usd: Decimal,
    pub acquisition_fee_usd: Decimal,
}

impl Disposal {
    pub fn basis_coverage(&self) -> Decimal {
        if self.quantity <= Decimal::ZERO {
            Decimal::ZERO
        } else {
            self.known_basis_quantity / self.quantity
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PositionSummary {
    pub current_quantity: Decimal,
    pub peak_quantity: Decimal,
    pub bought_quantity: Decimal,
    pub sold_quantity: Decimal,
    pub transferred_in_quantity: Decimal,
    pub transferred_out_quantity: Decimal,
    pub airdropped_quantity: Decimal,
    pub burned_quantity: Decimal,

    pub known_basis_quantity: Decimal,
    pub unknown_basis_quantity: Decimal,
    pub basis_coverage: Decimal,

    pub open_cost_usd_known: Decimal,
    pub open_acquisition_fees_usd: Decimal,
    pub average_entry_usd: Option<Decimal>,
    pub average_entry_usd_known_portion: Option<Decimal>,
    pub all_in_average_entry_usd: Option<Decimal>,

    pub realized_proceeds_usd: Decimal,
    pub realized_cost_usd_known: Decimal,
    pub realized_acquisition_fees_usd: Decimal,
    pub realized_disposal_fees_usd: Decimal,
    pub realized_pnl_usd: Option<Decimal>,
    pub realized_pnl_usd_known_portion: Decimal,
    pub realized_basis_coverage: Decimal,
    pub realized_proceeds_coverage: Decimal,

    pub first_acquired_at: Option<u64>,
    pub last_activity_at: Option<u64>,
}

#[derive(Debug, Error, PartialEq)]
pub enum LedgerError {
    #[error("quantity must be greater than zero")]
    NonPositiveQuantity,
    #[error("USD values and fees cannot be negative")]
    NegativeValue,
    #[error("disposal quantity {requested} exceeds reconstructed balance {available}")]
    InsufficientBalance {
        requested: Decimal,
        available: Decimal,
    },
    #[error("basis packet cannot be empty")]
    EmptyBasisPacket,
}

#[derive(Clone, Debug, Default)]
pub struct WalletLedger {
    lots: VecDeque<Lot>,
    current_quantity: Decimal,
    peak_quantity: Decimal,

    bought_quantity: Decimal,
    sold_quantity: Decimal,
    transferred_in_quantity: Decimal,
    transferred_out_quantity: Decimal,
    airdropped_quantity: Decimal,
    burned_quantity: Decimal,

    realized_proceeds_usd: Decimal,
    realized_cost_usd_known: Decimal,
    realized_acquisition_fees_usd: Decimal,
    realized_disposal_fees_usd: Decimal,
    realized_known_basis_quantity: Decimal,
    realized_unknown_basis_quantity: Decimal,
    realized_known_proceeds_quantity: Decimal,
    realized_unknown_proceeds_quantity: Decimal,
    realized_pnl_usd_known_portion: Decimal,

    first_acquired_at: Option<u64>,
    last_activity_at: Option<u64>,
}

impl WalletLedger {
    pub fn buy(
        &mut self,
        quantity: Decimal,
        trade_cost_usd: Decimal,
        network_fee_usd: Decimal,
        timestamp: u64,
        tx_id: impl Into<String>,
    ) -> Result<(), LedgerError> {
        validate_quantity(quantity)?;
        validate_value(trade_cost_usd)?;
        validate_value(network_fee_usd)?;

        self.push_lot(Lot {
            quantity,
            cost_usd: Some(trade_cost_usd),
            acquisition_fee_usd: network_fee_usd,
            acquired_at: timestamp,
            origin_tx_id: tx_id.into(),
        });

        self.bought_quantity += quantity;
        self.record_acquisition(quantity, timestamp);
        Ok(())
    }


    pub fn buy_unknown_cost(
        &mut self,
        quantity: Decimal,
        network_fee_usd: Decimal,
        timestamp: u64,
        tx_id: impl Into<String>,
    ) -> Result<(), LedgerError> {
        validate_quantity(quantity)?;
        validate_value(network_fee_usd)?;

        self.push_lot(Lot {
            quantity,
            cost_usd: None,
            acquisition_fee_usd: network_fee_usd,
            acquired_at: timestamp,
            origin_tx_id: tx_id.into(),
        });

        self.bought_quantity += quantity;
        self.record_acquisition(quantity, timestamp);
        Ok(())
    }

    pub fn airdrop(
        &mut self,
        quantity: Decimal,
        timestamp: u64,
        tx_id: impl Into<String>,
    ) -> Result<(), LedgerError> {
        validate_quantity(quantity)?;

        self.push_lot(Lot {
            quantity,
            cost_usd: Some(Decimal::ZERO),
            acquisition_fee_usd: Decimal::ZERO,
            acquired_at: timestamp,
            origin_tx_id: tx_id.into(),
        });

        self.airdropped_quantity += quantity;
        self.record_acquisition(quantity, timestamp);
        Ok(())
    }

    pub fn reconcile_unknown_in(
        &mut self,
        quantity: Decimal,
        timestamp: u64,
        tx_id: impl Into<String>,
    ) -> Result<(), LedgerError> {
        validate_quantity(quantity)?;

        self.push_lot(Lot {
            quantity,
            cost_usd: None,
            acquisition_fee_usd: Decimal::ZERO,
            acquired_at: timestamp,
            origin_tx_id: tx_id.into(),
        });

        self.transferred_in_quantity += quantity;
        self.record_acquisition(quantity, timestamp);
        Ok(())
    }

    pub fn transfer_in(
        &mut self,
        packet: BasisPacket,
        timestamp: u64,
    ) -> Result<(), LedgerError> {
        if packet.lots.is_empty() {
            return Err(LedgerError::EmptyBasisPacket);
        }

        let quantity = packet.quantity();
        validate_quantity(quantity)?;

        for carried in packet.lots {
            self.push_lot(Lot {
                quantity: carried.quantity,
                cost_usd: carried.cost_usd,
                acquisition_fee_usd: carried.acquisition_fee_usd,
                acquired_at: carried.acquired_at,
                origin_tx_id: carried.origin_tx_id,
            });
        }

        self.transferred_in_quantity += quantity;
        self.record_acquisition(quantity, timestamp);
        Ok(())
    }

    pub fn sell(
        &mut self,
        quantity: Decimal,
        proceeds_usd: Decimal,
        network_fee_usd: Decimal,
        timestamp: u64,
    ) -> Result<Disposal, LedgerError> {
        validate_quantity(quantity)?;
        validate_value(proceeds_usd)?;
        validate_value(network_fee_usd)?;

        let disposal = self.consume_fifo(quantity)?;

        let known_fraction = disposal.basis_coverage();
        let known_proceeds = proceeds_usd * known_fraction;
        let known_disposal_fee = network_fee_usd * known_fraction;
        let known_pnl = known_proceeds
            - disposal.known_cost_usd
            - disposal.acquisition_fee_usd
            - known_disposal_fee;

        self.current_quantity -= quantity;
        self.sold_quantity += quantity;
        self.realized_proceeds_usd += proceeds_usd;
        self.realized_cost_usd_known += disposal.known_cost_usd;
        self.realized_acquisition_fees_usd += disposal.acquisition_fee_usd;
        self.realized_disposal_fees_usd += network_fee_usd;
        self.realized_known_basis_quantity += disposal.known_basis_quantity;
        self.realized_unknown_basis_quantity += disposal.unknown_basis_quantity;
        self.realized_known_proceeds_quantity += quantity;
        self.realized_pnl_usd_known_portion += known_pnl;
        self.last_activity_at = Some(timestamp);

        Ok(disposal)
    }


    pub fn sell_unknown_proceeds(
        &mut self,
        quantity: Decimal,
        network_fee_usd: Decimal,
        timestamp: u64,
    ) -> Result<Disposal, LedgerError> {
        validate_quantity(quantity)?;
        validate_value(network_fee_usd)?;

        let disposal = self.consume_fifo(quantity)?;

        self.current_quantity -= quantity;
        self.sold_quantity += quantity;
        self.realized_cost_usd_known += disposal.known_cost_usd;
        self.realized_acquisition_fees_usd += disposal.acquisition_fee_usd;
        self.realized_disposal_fees_usd += network_fee_usd;
        self.realized_known_basis_quantity += disposal.known_basis_quantity;
        self.realized_unknown_basis_quantity += disposal.unknown_basis_quantity;
        self.realized_unknown_proceeds_quantity += quantity;
        self.last_activity_at = Some(timestamp);

        Ok(disposal)
    }

    pub fn transfer_out(
        &mut self,
        quantity: Decimal,
        timestamp: u64,
    ) -> Result<BasisPacket, LedgerError> {
        validate_quantity(quantity)?;
        let packet = self.consume_fifo_as_packet(quantity)?;

        self.current_quantity -= quantity;
        self.transferred_out_quantity += quantity;
        self.last_activity_at = Some(timestamp);

        Ok(packet)
    }

    pub fn burn(
        &mut self,
        quantity: Decimal,
        timestamp: u64,
    ) -> Result<Disposal, LedgerError> {
        validate_quantity(quantity)?;
        let disposal = self.consume_fifo(quantity)?;

        self.current_quantity -= quantity;
        self.burned_quantity += quantity;
        self.last_activity_at = Some(timestamp);

        Ok(disposal)
    }

    pub fn summary(&self) -> PositionSummary {
        let mut known_basis_quantity = Decimal::ZERO;
        let mut unknown_basis_quantity = Decimal::ZERO;
        let mut open_cost_usd_known = Decimal::ZERO;
        let mut open_acquisition_fees_usd = Decimal::ZERO;

        for lot in &self.lots {
            match lot.cost_usd {
                Some(cost) => {
                    known_basis_quantity += lot.quantity;
                    open_cost_usd_known += cost;
                    open_acquisition_fees_usd += lot.acquisition_fee_usd;
                }
                None => {
                    unknown_basis_quantity += lot.quantity;
                }
            }
        }

        let basis_coverage = ratio(known_basis_quantity, self.current_quantity);
        let average_entry_known_portion = nonzero_div(open_cost_usd_known, known_basis_quantity);
        let average_entry_usd =
            if unknown_basis_quantity == Decimal::ZERO && self.current_quantity > Decimal::ZERO {
                nonzero_div(open_cost_usd_known, self.current_quantity)
            } else {
                None
            };
        let all_in_average_entry_usd =
            if unknown_basis_quantity == Decimal::ZERO && self.current_quantity > Decimal::ZERO {
                nonzero_div(
                    open_cost_usd_known + open_acquisition_fees_usd,
                    self.current_quantity,
                )
            } else {
                None
            };

        let realized_quantity =
            self.realized_known_basis_quantity + self.realized_unknown_basis_quantity;
        let realized_basis_coverage = ratio(self.realized_known_basis_quantity, realized_quantity);
        let realized_proceeds_coverage =
            ratio(self.realized_known_proceeds_quantity, realized_quantity);
        let realized_pnl_usd = if self.realized_unknown_basis_quantity == Decimal::ZERO
            && self.realized_unknown_proceeds_quantity == Decimal::ZERO
        {
            Some(
                self.realized_proceeds_usd
                    - self.realized_cost_usd_known
                    - self.realized_acquisition_fees_usd
                    - self.realized_disposal_fees_usd,
            )
        } else {
            None
        };

        PositionSummary {
            current_quantity: self.current_quantity,
            peak_quantity: self.peak_quantity,
            bought_quantity: self.bought_quantity,
            sold_quantity: self.sold_quantity,
            transferred_in_quantity: self.transferred_in_quantity,
            transferred_out_quantity: self.transferred_out_quantity,
            airdropped_quantity: self.airdropped_quantity,
            burned_quantity: self.burned_quantity,

            known_basis_quantity,
            unknown_basis_quantity,
            basis_coverage,

            open_cost_usd_known,
            open_acquisition_fees_usd,
            average_entry_usd,
            average_entry_usd_known_portion: average_entry_known_portion,
            all_in_average_entry_usd,

            realized_proceeds_usd: self.realized_proceeds_usd,
            realized_cost_usd_known: self.realized_cost_usd_known,
            realized_acquisition_fees_usd: self.realized_acquisition_fees_usd,
            realized_disposal_fees_usd: self.realized_disposal_fees_usd,
            realized_pnl_usd,
            realized_pnl_usd_known_portion: self.realized_pnl_usd_known_portion,
            realized_basis_coverage,
            realized_proceeds_coverage,

            first_acquired_at: self.first_acquired_at,
            last_activity_at: self.last_activity_at,
        }
    }

    fn record_acquisition(&mut self, quantity: Decimal, timestamp: u64) {
        self.current_quantity += quantity;
        if self.current_quantity > self.peak_quantity {
            self.peak_quantity = self.current_quantity;
        }
        self.first_acquired_at = Some(
            self.first_acquired_at
                .map(|existing| existing.min(timestamp))
                .unwrap_or(timestamp),
        );
        self.last_activity_at = Some(timestamp);
    }

    fn push_lot(&mut self, lot: Lot) {
        self.lots.push_back(lot);
    }

    fn ensure_balance(&self, quantity: Decimal) -> Result<(), LedgerError> {
        if quantity > self.current_quantity {
            Err(LedgerError::InsufficientBalance {
                requested: quantity,
                available: self.current_quantity,
            })
        } else {
            Ok(())
        }
    }

    fn consume_fifo(&mut self, quantity: Decimal) -> Result<Disposal, LedgerError> {
        self.ensure_balance(quantity)?;

        let packet = self.consume_fifo_as_packet_unchecked(quantity);
        let mut known_basis_quantity = Decimal::ZERO;
        let mut unknown_basis_quantity = Decimal::ZERO;
        let mut known_cost_usd = Decimal::ZERO;
        let mut acquisition_fee_usd = Decimal::ZERO;

        for lot in packet.lots {
            match lot.cost_usd {
                Some(cost) => {
                    known_basis_quantity += lot.quantity;
                    known_cost_usd += cost;
                    acquisition_fee_usd += lot.acquisition_fee_usd;
                }
                None => {
                    unknown_basis_quantity += lot.quantity;
                }
            }
        }

        Ok(Disposal {
            quantity,
            known_basis_quantity,
            unknown_basis_quantity,
            known_cost_usd,
            acquisition_fee_usd,
        })
    }

    fn consume_fifo_as_packet(
        &mut self,
        quantity: Decimal,
    ) -> Result<BasisPacket, LedgerError> {
        self.ensure_balance(quantity)?;
        Ok(self.consume_fifo_as_packet_unchecked(quantity))
    }

    fn consume_fifo_as_packet_unchecked(&mut self, quantity: Decimal) -> BasisPacket {
        let mut remaining = quantity;
        let mut carried = Vec::new();

        while remaining > Decimal::ZERO {
            let mut lot = self
                .lots
                .pop_front()
                .expect("balance invariant guarantees a lot exists");

            let take = if lot.quantity <= remaining {
                lot.quantity
            } else {
                remaining
            };

            let original_quantity = lot.quantity;
            let fraction = take / original_quantity;
            let carried_cost = lot.cost_usd.map(|cost| cost * fraction);
            let carried_fee = lot.acquisition_fee_usd * fraction;

            carried.push(CarriedLot {
                quantity: take,
                cost_usd: carried_cost,
                acquisition_fee_usd: carried_fee,
                acquired_at: lot.acquired_at,
                origin_tx_id: lot.origin_tx_id.clone(),
            });

            lot.quantity -= take;
            if let Some(cost) = lot.cost_usd.as_mut() {
                *cost -= carried_cost.unwrap_or(Decimal::ZERO);
            }
            lot.acquisition_fee_usd -= carried_fee;

            remaining -= take;

            if lot.quantity > Decimal::ZERO {
                self.lots.push_front(lot);
            }
        }

        BasisPacket { lots: carried }
    }
}

fn validate_quantity(quantity: Decimal) -> Result<(), LedgerError> {
    if quantity <= Decimal::ZERO {
        Err(LedgerError::NonPositiveQuantity)
    } else {
        Ok(())
    }
}

fn validate_value(value: Decimal) -> Result<(), LedgerError> {
    if value < Decimal::ZERO {
        Err(LedgerError::NegativeValue)
    } else {
        Ok(())
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
    (denominator > Decimal::ZERO).then_some(numerator / denominator)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(value: i64) -> Decimal {
        Decimal::from(value)
    }

    #[test]
    fn fifo_sale_preserves_remaining_lot_entry() {
        let mut ledger = WalletLedger::default();
        ledger.buy(d(100), d(100), d(1), 10, "buy-a").unwrap();
        ledger.buy(d(100), d(300), d(1), 20, "buy-b").unwrap();

        let sold = ledger.sell(d(100), d(250), d(1), 30).unwrap();
        let summary = ledger.summary();

        assert_eq!(sold.known_cost_usd, d(100));
        assert_eq!(summary.current_quantity, d(100));
        assert_eq!(summary.average_entry_usd, Some(d(3)));
        assert_eq!(summary.realized_pnl_usd, Some(d(148)));
    }

    #[test]
    fn transfer_carries_original_basis_without_realizing_pnl() {
        let mut source = WalletLedger::default();
        source.buy(d(100), d(200), d(2), 10, "origin").unwrap();

        let packet = source.transfer_out(d(40), 20).unwrap();
        let mut destination = WalletLedger::default();
        destination.transfer_in(packet, 20).unwrap();

        let source_summary = source.summary();
        let destination_summary = destination.summary();

        assert_eq!(source_summary.current_quantity, d(60));
        assert_eq!(source_summary.realized_proceeds_usd, Decimal::ZERO);
        assert_eq!(destination_summary.current_quantity, d(40));
        assert_eq!(destination_summary.average_entry_usd, Some(d(2)));
        assert_eq!(destination_summary.first_acquired_at, Some(10));
    }

    #[test]
    fn unknown_inbound_prevents_fake_average_entry() {
        let mut ledger = WalletLedger::default();
        ledger.buy(d(50), d(100), Decimal::ZERO, 10, "buy").unwrap();
        ledger
            .reconcile_unknown_in(d(50), 20, "unknown-transfer")
            .unwrap();

        let summary = ledger.summary();

        assert_eq!(summary.current_quantity, d(100));
        assert_eq!(summary.basis_coverage, Decimal::new(5, 1));
        assert_eq!(summary.average_entry_usd, None);
        assert_eq!(summary.average_entry_usd_known_portion, Some(d(2)));
    }

    #[test]
    fn sale_with_unknown_basis_does_not_invent_total_realized_pnl() {
        let mut ledger = WalletLedger::default();
        ledger
            .reconcile_unknown_in(d(100), 10, "unknown-transfer")
            .unwrap();
        ledger.sell(d(100), d(500), d(1), 20).unwrap();

        let summary = ledger.summary();

        assert_eq!(summary.realized_basis_coverage, Decimal::ZERO);
        assert_eq!(summary.realized_pnl_usd, None);
    }

    #[test]
    fn refuses_to_silently_go_negative() {
        let mut ledger = WalletLedger::default();
        ledger.buy(d(10), d(10), Decimal::ZERO, 10, "buy").unwrap();

        let error = ledger.sell(d(11), d(20), Decimal::ZERO, 20).unwrap_err();

        assert_eq!(
            error,
            LedgerError::InsufficientBalance {
                requested: d(11),
                available: d(10),
            }
        );
    }


    #[test]
    fn unknown_cost_buy_is_a_trade_but_does_not_fake_entry() {
        let mut ledger = WalletLedger::default();
        ledger
            .buy_unknown_cost(d(25), Decimal::ZERO, 10, "priced-later")
            .unwrap();

        let summary = ledger.summary();

        assert_eq!(summary.bought_quantity, d(25));
        assert_eq!(summary.basis_coverage, Decimal::ZERO);
        assert_eq!(summary.average_entry_usd, None);
    }

    #[test]
    fn unknown_sale_proceeds_keep_distribution_but_hide_realized_pnl() {
        let mut ledger = WalletLedger::default();
        ledger.buy(d(100), d(100), Decimal::ZERO, 10, "buy").unwrap();
        ledger
            .sell_unknown_proceeds(d(50), Decimal::ZERO, 20)
            .unwrap();

        let summary = ledger.summary();

        assert_eq!(summary.sold_quantity, d(50));
        assert_eq!(summary.realized_proceeds_coverage, Decimal::ZERO);
        assert_eq!(summary.realized_pnl_usd, None);
    }

}
