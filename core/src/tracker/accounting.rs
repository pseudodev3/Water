use super::model::*;
use crate::ledger::WalletLedger;
use rust_decimal::Decimal;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Episode {
    pnl: Decimal,
    fees: Decimal,
    known: bool,
    traded: bool,
}
struct Closed {
    time: u64,
    asset: String,
    pnl: Decimal,
}

pub struct PriceIndex {
    exact: BTreeMap<(String, u64), Decimal>,
    hour: BTreeMap<(String, u64), Decimal>,
    day: BTreeMap<(String, u64), Decimal>,
}
impl PriceIndex {
    pub fn new(prices: &[Price]) -> Self {
        let mut index = Self {
            exact: BTreeMap::new(),
            hour: BTreeMap::new(),
            day: BTreeMap::new(),
        };
        for p in prices {
            index.exact.insert((p.asset.clone(), p.timestamp), p.usd);
            match p.granularity.as_str() {
                "hour" => {
                    index
                        .hour
                        .insert((p.asset.clone(), p.timestamp / 3600), p.usd);
                }
                "day" => {
                    index
                        .day
                        .insert((p.asset.clone(), p.timestamp / DAY), p.usd);
                }
                _ => {}
            }
        }
        index
    }
    pub fn get(&self, asset: &str, time: u64) -> Option<Decimal> {
        self.exact
            .get(&(asset.into(), time))
            .or_else(|| self.hour.get(&(asset.into(), time / 3600)))
            .or_else(|| self.day.get(&(asset.into(), time / DAY)))
            .copied()
            .filter(|p| *p > Decimal::ZERO)
    }
}
fn price(prices: &PriceIndex, asset: &str, time: u64) -> Option<Decimal> {
    prices.get(asset, time)
}

fn fee(prices: &PriceIndex, tx: &Transaction) -> Option<Decimal> {
    let quantity = tx.fee_quantity?;
    if quantity == Decimal::ZERO {
        return Some(Decimal::ZERO);
    }
    Some(quantity * price(prices, &tx.fee_asset, tx.timestamp)?)
}

fn open_total(
    ledgers: &BTreeMap<String, WalletLedger>,
    prices: &PriceIndex,
    timestamp: u64,
) -> Option<Decimal> {
    let mut open = Some(Decimal::ZERO);
    for (asset, ledger) in ledgers {
        let position = ledger.summary();
        if position.current_quantity > Decimal::ZERO {
            open = open
                .zip(price(prices, asset, timestamp))
                .filter(|_| position.basis_coverage == Decimal::ONE)
                .map(|(a, p)| a + position.current_quantity * p - position.open_cost_usd_known);
        }
    }
    open
}

pub fn analyze(mut snapshot: Snapshot, timestamp: u64) -> Analysis {
    let chain = snapshot.candidate.chain;
    let prices = PriceIndex::new(&snapshot.prices);
    let end = timestamp;
    let start = end.saturating_sub(60 * DAY);
    let split = end.saturating_sub(30 * DAY);
    let boundaries = [start, split, end];
    let mut transactions: Vec<_> = snapshot
        .records
        .iter()
        .filter_map(|r| r.transaction.clone())
        .filter(|t| t.timestamp < end)
        .collect();
    transactions.sort_by_key(|t| (t.block, t.index.unwrap_or(u64::MAX), t.id.clone()));
    let mut ledgers: BTreeMap<String, WalletLedger> = BTreeMap::new();
    let mut episodes: BTreeMap<String, Episode> = BTreeMap::new();
    let mut closed = Vec::new();
    let mut events = Vec::new();
    let mut economic_gaps = 0;
    let mut marks = Vec::new();
    let mut boundary_index = 0;
    let mut days = BTreeSet::new();
    let mut fee_rows = Vec::new();
    let mut sales = Vec::new();
    for tx in &transactions {
        while boundary_index < boundaries.len() && tx.timestamp >= boundaries[boundary_index] {
            marks.push(open_total(&ledgers, &prices, boundaries[boundary_index]));
            boundary_index += 1;
        }
        if !tx.finalized {
            economic_gaps += 1;
            events.push(Activity {
                tx: tx.id.clone(),
                timestamp: tx.timestamp,
                kind: "provisional_transaction".into(),
                asset: None,
                quantity: None,
                quote_asset: None,
                quote_quantity: None,
                value_usd: None,
                finalized: false,
                counterparties: tx.counterparties.clone(),
            });
            continue;
        }
        let fee_usd = fee(&prices, tx);
        fee_rows.push((tx.timestamp, fee_usd));
        if !tx.movement_complete {
            economic_gaps += 1;
            events.push(Activity {
                tx: tx.id.clone(),
                timestamp: tx.timestamp,
                kind: "unresolved_execution".into(),
                asset: None,
                quantity: None,
                quote_asset: None,
                quote_quantity: None,
                value_usd: None,
                finalized: tx.finalized,
                counterparties: tx.counterparties.clone(),
            });
        }
        if !tx.succeeded {
            events.push(Activity {
                tx: tx.id.clone(),
                timestamp: tx.timestamp,
                kind: "failed_transaction".into(),
                asset: None,
                quantity: None,
                quote_asset: None,
                quote_quantity: None,
                value_usd: fee_usd,
                finalized: true,
                counterparties: vec![],
            });
            continue;
        }
        let targets: Vec<_> = tx
            .assets
            .iter()
            .filter(|d| !quote(chain, &d.asset) && d.quantity != Decimal::ZERO)
            .collect();
        let quotes: Vec<_> = tx
            .assets
            .iter()
            .filter(|d| quote(chain, &d.asset) && d.quantity != Decimal::ZERO)
            .collect();
        if targets.len() > 1 {
            economic_gaps += 1;
        }
        for target in targets.iter() {
            let ledger = ledgers.entry(target.asset.clone()).or_default();
            let before = ledger.summary();
            let opposite: Vec<_> = quotes
                .iter()
                .filter(|q| (q.quantity > Decimal::ZERO) != (target.quantity > Decimal::ZERO))
                .collect();
            let trade = targets.len() == 1
                && opposite.len() == 1
                && tx.swap_evidence
                && tx.movement_complete;
            let quote = trade.then(|| opposite[0]);
            let usd = quote
                .and_then(|q| price(&prices, &q.asset, tx.timestamp).map(|p| q.quantity.abs() * p));
            let quantity = target.quantity.abs();
            let episode = episodes.entry(target.asset.clone()).or_default();
            if before.current_quantity == Decimal::ZERO {
                *episode = Episode {
                    known: true,
                    ..Default::default()
                };
            }
            let kind;
            let applied = if trade && target.quantity > Decimal::ZERO {
                kind = if before.current_quantity == Decimal::ZERO {
                    "entry"
                } else {
                    "addition"
                };
                episode.traded = true;
                episode.known &= usd.is_some() && fee_usd.is_some();
                episode.fees += fee_usd.unwrap_or_default();
                days.insert((tx.timestamp / DAY, target.asset.clone()));
                if let Some(cost) = usd {
                    ledger.buy(quantity, cost, Decimal::ZERO, tx.timestamp, &tx.id)
                } else {
                    ledger.buy_unknown_cost(quantity, Decimal::ZERO, tx.timestamp, &tx.id)
                }
            } else if trade {
                kind = if before.current_quantity == quantity {
                    "exit"
                } else {
                    "partial_exit"
                };
                episode.traded = true;
                episode.known &= usd.is_some() && fee_usd.is_some();
                episode.fees += fee_usd.unwrap_or_default();
                days.insert((tx.timestamp / DAY, target.asset.clone()));
                let disposal = if let Some(proceeds) = usd {
                    ledger.sell(quantity, proceeds, Decimal::ZERO, tx.timestamp)
                } else {
                    ledger.sell_unknown_proceeds(quantity, Decimal::ZERO, tx.timestamp)
                };
                // A sale's FIFO basis is independent of unrelated, already
                // closed lifetime trades whose historical USD price is unknown.
                let pnl = disposal.as_ref().ok().and_then(|d| {
                    usd.filter(|_| d.unknown_basis_quantity == Decimal::ZERO)
                        .map(|proceeds| proceeds - d.known_cost_usd - d.acquisition_fee_usd)
                });
                sales.push((tx.timestamp, pnl));
                episode.known &= pnl.is_some();
                episode.pnl += pnl.unwrap_or_default();
                disposal.map(|_| ())
            } else if target.quantity > Decimal::ZERO {
                kind = "transfer_in";
                episode.known = false;
                if !opposite.is_empty() {
                    economic_gaps += 1;
                }
                ledger.reconcile_unknown_in(quantity, tx.timestamp, &tx.id)
            } else {
                kind = "transfer_out";
                episode.known = false;
                // Unproven ownership/basis carry must not let withdrawing a
                // losing token erase that loss from the portfolio result.
                economic_gaps += 1;
                ledger.transfer_out(quantity, tx.timestamp).map(|_| ())
            };
            if applied.is_err() {
                economic_gaps += 1;
            }
            let after = ledger.summary();
            if after.current_quantity == Decimal::ZERO
                && episode.traded
                && episode.known
                && kind == "exit"
                && applied.is_ok()
            {
                closed.push(Closed {
                    time: tx.timestamp,
                    asset: target.asset.clone(),
                    pnl: episode.pnl - episode.fees,
                });
            }
            events.push(Activity {
                tx: tx.id.clone(),
                timestamp: tx.timestamp,
                kind: kind.into(),
                asset: Some(target.asset.clone()),
                quantity: Some(quantity),
                quote_asset: quote.map(|q| q.asset.clone()),
                quote_quantity: quote.map(|q| q.quantity.abs()),
                value_usd: usd,
                finalized: tx.finalized,
                counterparties: tx.counterparties.clone(),
            });
        }
        if targets.is_empty() && !tx.assets.is_empty() {
            // Quote-to-quote trading needs its own basis ledger. Exact native /
            // wrapped-native conversions are the sole neutral exception.
            if tx.assets.len() > 1 {
                let neutral_wrap = !matches!(chain, crate::model::Chain::Solana)
                    && tx.assets.len() == 2
                    && tx.assets.iter().all(|d| match chain {
                        crate::model::Chain::Robinhood => quote(chain, &d.asset),
                        crate::model::Chain::Bnb => {
                            d.asset.eq_ignore_ascii_case("BNB")
                                || d.asset.eq_ignore_ascii_case(
                                    "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c",
                                )
                        }
                        _ => false,
                    })
                    && tx.assets.iter().map(|d| d.quantity).sum::<Decimal>() == Decimal::ZERO;
                if !neutral_wrap {
                    economic_gaps += 1;
                }
            }
            for delta in &tx.assets {
                events.push(Activity {
                    tx: tx.id.clone(),
                    timestamp: tx.timestamp,
                    kind: if tx.assets.len() > 1 {
                        "quote_conversion"
                    } else if delta.quantity > Decimal::ZERO {
                        "funding_in"
                    } else {
                        "funding_out"
                    }
                    .into(),
                    asset: Some(delta.asset.clone()),
                    quantity: Some(delta.quantity.abs()),
                    quote_asset: None,
                    quote_quantity: None,
                    value_usd: price(&prices, &delta.asset, tx.timestamp)
                        .map(|p| p * delta.quantity.abs()),
                    finalized: tx.finalized,
                    counterparties: tx.counterparties.clone(),
                });
            }
        }
    }
    while boundary_index < boundaries.len() {
        marks.push(open_total(&ledgers, &prices, boundaries[boundary_index]));
        boundary_index += 1;
    }
    let unparsed = snapshot
        .records
        .iter()
        .filter(|r| r.transaction.is_none())
        .count();
    economic_gaps += unparsed;
    let mut coverage = snapshot.coverage.clone();
    coverage.oldest_record_at = transactions.iter().map(|t| t.timestamp).min();
    coverage.newest_record_at = transactions.iter().map(|t| t.timestamp).max();
    coverage.pending_records = unparsed
        + transactions
            .iter()
            .filter(|t| !t.finalized || !t.movement_complete)
            .count();
    coverage.fees_complete = fee_rows
        .iter()
        .filter(|(t, _)| *t >= start)
        .all(|(_, v)| v.is_some());
    let mut reconciliation = coverage.balances_reconciled;
    for (asset, ledger) in &ledgers {
        if snapshot.balances.get(asset).copied().unwrap_or_default()
            != ledger.summary().current_quantity
        {
            reconciliation = false;
        }
    }
    for (asset, balance) in &snapshot.balances {
        if !quote(chain, asset)
            && *balance
                != ledgers
                    .get(asset)
                    .map(|l| l.summary().current_quantity)
                    .unwrap_or_default()
        {
            reconciliation = false;
        }
    }
    coverage.balances_reconciled = reconciliation;
    let mut windows = Vec::new();
    let traded: Vec<_> = events
        .iter()
        .filter(|e| {
            e.finalized
                && matches!(
                    e.kind.as_str(),
                    "entry" | "addition" | "partial_exit" | "exit"
                )
        })
        .map(|e| e.timestamp)
        .collect();
    let first_trade = traded.iter().copied().min();
    let recent = traded
        .iter()
        .copied()
        .max()
        .is_some_and(|t| t >= end.saturating_sub(7 * DAY));
    let fresh = coverage
        .last_collected_at
        .is_some_and(|t| end.saturating_sub(t) <= 3600);
    let state_fresh = coverage
        .last_state_checked_at
        .is_some_and(|t| end.saturating_sub(t) <= 3600);
    for i in 0..2 {
        let (from, to) = (boundaries[i], boundaries[i + 1]);
        let complete_economics = coverage.history_complete
            && coverage.head_complete
            && coverage.ordering_complete
            && coverage.balances_reconciled
            && state_fresh
            && economic_gaps == 0
            && coverage.fees_complete;
        let realized = if complete_economics {
            sales
                .iter()
                .filter(|(t, _)| *t >= from && *t < to)
                .try_fold(Decimal::ZERO, |sum, (_, pnl)| pnl.map(|p| sum + p))
        } else {
            None
        };
        let open_change = if complete_economics {
            marks[i + 1].zip(marks[i]).map(|(b, a)| b - a)
        } else {
            None
        };
        let costs = if complete_economics {
            fee_rows
                .iter()
                .filter(|(t, _)| *t >= from && *t < to)
                .try_fold(Decimal::ZERO, |s, (_, f)| f.map(|v| s + v))
        } else {
            None
        };
        let result = realized
            .zip(open_change)
            .zip(costs)
            .map(|((a, b), c)| a + b - c);
        let sample: Vec<_> = closed
            .iter()
            .filter(|c| c.time >= from && c.time < to)
            .collect();
        let gains: Decimal = sample
            .iter()
            .filter(|c| c.pnl > Decimal::ZERO)
            .map(|c| c.pnl)
            .sum();
        let losses: Decimal = sample
            .iter()
            .filter(|c| c.pnl < Decimal::ZERO)
            .map(|c| -c.pnl)
            .sum();
        let factor = (losses > Decimal::ZERO).then(|| gains / losses);
        let largest = sample
            .iter()
            .map(|c| c.pnl)
            .filter(|p| *p > Decimal::ZERO)
            .max();
        let after_fees = realized.zip(costs).map(|(p, f)| p - f);
        let without = after_fees.zip(largest).map(|(p, w)| p - w);
        let share = largest.filter(|_| gains > Decimal::ZERO).map(|w| w / gains);
        let tokens = sample
            .iter()
            .map(|c| &c.asset)
            .collect::<BTreeSet<_>>()
            .len();
        let active_days = days
            .iter()
            .filter(|(d, _)| *d >= from / DAY && *d <= (to - 1) / DAY)
            .map(|(d, _)| d)
            .collect::<BTreeSet<_>>()
            .len();
        let mut gates = Vec::new();
        let mut gate = |name: &str, passed: bool, detail: String| {
            gates.push(Gate {
                name: name.into(),
                passed,
                detail,
            })
        };
        gate("Complete history",complete_economics&&coverage.backfill_done&&realized.is_some()&&open_change.is_some(),"Full wallet history, canonical order, ending balances, opening lots, USD valuations and fees must reconcile.".into());
        gate("Execution account",coverage.execution_account_verified,"Standard wallet or verified EIP-7702 delegation; other contract/program accounts need an ownership and fee adapter.".into());
        gate(
            "Record age",
            first_trade.is_some_and(|t| t <= from),
            "Verified trading activity must predate the start of this 30-day window.".into(),
        );
        gate(
            "Profitable window",
            after_fees.is_some_and(|p| p > Decimal::ZERO)
                && result.is_some_and(|p| p > Decimal::ZERO),
            "Realized and total results must be positive after including open losses and fees."
                .into(),
        );
        gate("Sufficient record",sample.len()>=30&&tokens>=10&&active_days>=15,format!("{} completed episodes, {tokens} tokens, {active_days} active days; requires 30 / 10 / 15.",sample.len()));
        gate("Profit factor",factor.is_some_and(|p|p>=Decimal::new(15,1)),"Gross winning / losing episode PnL must be at least 1.5; no-loss denominator stays undefined.".into());
        gate("Outlier independence",without.is_some_and(|p|p>Decimal::ZERO)&&share.is_some_and(|s|s<=Decimal::new(35,2)),"Positive without the largest winner; that winner contributes at most 35% of gross gains.".into());
        gate(
            "Recent and fresh",
            recent && fresh && state_fresh,
            "A trade in seven days, current history in one hour, and account/balance state checked in one hour are required.".into(),
        );
        let qualified = gates.iter().all(|g| g.passed);
        windows.push(Window {
            start: from,
            end: to,
            realized_usd: realized,
            open_change_usd: open_change,
            fees_usd: costs,
            total_usd: result,
            episodes: sample.len(),
            tokens,
            active_days,
            wins: sample.iter().filter(|c| c.pnl > Decimal::ZERO).count(),
            losses: sample.iter().filter(|c| c.pnl < Decimal::ZERO).count(),
            profit_factor: factor,
            largest_winner_usd: largest,
            profit_without_largest_usd: without,
            largest_profit_share: share,
            gates,
            qualified,
        });
    }
    let positions = ledgers
        .iter()
        .map(|(asset, ledger)| {
            let p = ledger.summary();
            Position {
                asset: asset.clone(),
                quantity: p.current_quantity,
                known_cost_usd: p.open_cost_usd_known,
                basis_coverage: p.basis_coverage,
                market_value_usd: if p.current_quantity == Decimal::ZERO {
                    Some(Decimal::ZERO)
                } else {
                    price(&prices, asset, end).map(|v| v * p.current_quantity)
                },
                realized_usd: p.realized_pnl_usd,
                first_acquired_at: p.first_acquired_at,
                last_activity_at: p.last_activity_at,
            }
        })
        .collect();
    events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    events.truncate(100);
    let status = if !fresh && coverage.last_collected_at.is_some() {
        "stale"
    } else if windows.iter().all(|w| w.qualified) {
        "qualified_60d"
    } else if windows[1].qualified {
        "qualified_30d"
    } else if !coverage.history_complete
        || economic_gaps > 0
        || !coverage.balances_reconciled
        || !coverage.execution_account_verified
        || !coverage.fees_complete
        || windows.iter().any(|w| w.total_usd.is_none())
    {
        "incomplete"
    } else {
        "observed"
    };
    snapshot.coverage = coverage.clone();
    Analysis{candidate:snapshot.candidate,analyzed_at:end,policy:POLICY.into(),status:status.into(),coverage,windows,positions,activity:events,unresolved_records:economic_gaps,records:snapshot.records.len(),notes:vec!["Historical USD values use source candles at the event/boundary time; conversion is an estimate, not an executable quote.".into(),"Network fees are expensed when charged. Deposits, withdrawals and unproven transfer basis are not trading gains.".into(),"Initial qualification thresholds are research filters; future profitability is evaluated separately.".into()]}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Chain;
    fn base() -> Snapshot {
        Snapshot {
            candidate: Candidate {
                observed_tokens: Vec::new(),
                chain: Chain::Solana,
                wallet: "test".into(),
                discovered_at: 0,
                sources: vec![],
            },
            coverage: Coverage {
                history_complete: true,
                backfill_done: true,
                head_complete: true,
                ordering_complete: true,
                balances_reconciled: true,
                execution_account_verified: true,
                last_collected_at: Some(90 * DAY),
                last_state_checked_at: Some(90 * DAY),
                ..Default::default()
            },
            records: vec![],
            prices: vec![],
            balances: BTreeMap::new(),
        }
    }
    fn tx(id: &str, time: u64, token: i64, sol: i64, swap: bool) -> Record {
        Record {
            id: id.into(),
            raw: serde_json::json!({"synthetic":true}),
            error: None,
            transaction: Some(Transaction {
                id: id.into(),
                timestamp: time,
                block: time,
                index: Some(0),
                finalized: true,
                succeeded: true,
                assets: vec![
                    Delta {
                        asset: "token".into(),
                        quantity: Decimal::from(token),
                    },
                    Delta {
                        asset: "SOL".into(),
                        quantity: Decimal::from(sol),
                    },
                ],
                fee_asset: "SOL".into(),
                fee_quantity: Some(Decimal::ZERO),
                movement_complete: true,
                swap_evidence: swap,
                notes: vec![],
                counterparties: vec![],
            }),
        }
    }
    fn prices(s: &mut Snapshot, times: &[u64]) {
        for t in times {
            s.prices.push(Price {
                asset: "SOL".into(),
                timestamp: *t,
                usd: Decimal::from(100),
                granularity: "test".into(),
                source: "synthetic test only".into(),
            });
        }
    }
    #[test]
    fn opening_lots_and_partial_exits_use_fifo_once() {
        let mut s = base();
        s.records = vec![
            tx("buy", 10 * DAY, 100, -1, true),
            tx("partial", 70 * DAY, -40, 1, true),
            tx("exit", 80 * DAY, -60, 1, true),
        ];
        prices(&mut s, &[10 * DAY, 70 * DAY, 80 * DAY]);
        s.prices.push(Price {
            asset: "token".into(),
            timestamp: 30 * DAY,
            usd: Decimal::ONE,
            granularity: "test".into(),
            source: "test".into(),
        });
        s.prices.push(Price {
            asset: "token".into(),
            timestamp: 60 * DAY,
            usd: Decimal::ONE,
            granularity: "test".into(),
            source: "test".into(),
        });
        let a = analyze(s, 90 * DAY);
        assert_eq!(a.windows[1].episodes, 1);
        assert_eq!(a.windows[1].realized_usd, Some(Decimal::from(100)));
        assert_eq!(a.positions[0].quantity, Decimal::ZERO);
        assert_ne!(a.status, "qualified_30d");
    }
    #[test]
    fn transfer_income_missing_fees_and_open_losses_never_qualify() {
        let mut s = base();
        s.records = vec![
            tx("transfer", 65 * DAY, 100, 0, false),
            tx("sell", 70 * DAY, -100, 2, true),
        ];
        prices(&mut s, &[70 * DAY]);
        let a = analyze(s, 90 * DAY);
        assert_eq!(a.windows[1].realized_usd, None);
        assert_eq!(a.windows[1].episodes, 0);
        assert!(!a.windows[1].qualified);
        let mut s = base();
        let mut r = tx("buy", 70 * DAY, 100, -1, true);
        r.transaction.as_mut().unwrap().fee_quantity = None;
        s.records.push(r);
        prices(&mut s, &[70 * DAY]);
        s.balances.insert("token".into(), Decimal::from(100));
        let a = analyze(s, 90 * DAY);
        assert_eq!(a.windows[1].fees_usd, None);
        assert_eq!(a.windows[1].open_change_usd, None);
        assert!(!a.windows[1].qualified);
    }
    #[test]
    fn stale_or_capped_records_cannot_be_promoted() {
        let mut s = base();
        s.coverage.history_complete = false;
        s.coverage.last_collected_at = Some(80 * DAY);
        let a = analyze(s, 90 * DAY);
        assert_eq!(a.status, "stale");
        assert!(a.windows.iter().all(|w| !w.qualified));
    }

    #[test]
    fn fresh_history_does_not_promote_old_account_and_balance_state() {
        let mut s = consistent_sample();
        assert_eq!(analyze(s.clone(), 90 * DAY).status, "qualified_60d");
        s.coverage.last_state_checked_at = Some(88 * DAY);
        let a = analyze(s, 90 * DAY);
        assert!(!a.status.starts_with("qualified_"));
        assert!(a
            .windows
            .iter()
            .all(|w| !w.qualified && w.total_usd.is_none()));
    }
    fn consistent_sample() -> Snapshot {
        let mut s = base();
        s.records.extend([
            tx("age-buy", 29 * DAY, 1, -1, true),
            tx("age-sell", 29 * DAY + 30, -1, 2, true),
        ]);
        for at in [29 * DAY, 29 * DAY + 30] {
            s.prices.push(Price {
                asset: "SOL".into(),
                timestamp: at,
                usd: Decimal::ONE,
                granularity: "test".into(),
                source: "synthetic test only".into(),
            });
        }
        for month in [30u64, 60] {
            for n in 0..32u64 {
                let time = (month + 11 + n / 2) * DAY + n % 2 * 60;
                let asset = format!("token{}", n % 10);
                for (suffix, at, quantity, sol) in [
                    ("buy", time, 10, -10),
                    ("sell", time + 30, -10, if n < 25 { 12 } else { 9 }),
                ] {
                    let mut record = tx(&format!("{month}-{n}-{suffix}"), at, quantity, sol, true);
                    record.transaction.as_mut().unwrap().assets[0].asset = asset.clone();
                    s.records.push(record);
                    s.prices.push(Price {
                        asset: "SOL".into(),
                        timestamp: at,
                        usd: Decimal::ONE,
                        granularity: "test".into(),
                        source: "synthetic test only".into(),
                    });
                }
            }
        }
        s
    }
    #[test]
    fn both_independent_months_can_qualify_and_outliers_cannot() {
        let s = consistent_sample();
        let a = analyze(s.clone(), 90 * DAY);
        assert_eq!(a.status, "qualified_60d", "{:?}", a.windows);
        assert!(a.windows.iter().all(|w| w.episodes == 32
            && w.tokens == 10
            && w.active_days == 16
            && w.total_usd == Some(Decimal::from(43))));
        let mut s = s;
        let row = s.records.iter_mut().find(|r| r.id == "60-0-sell").unwrap();
        row.transaction.as_mut().unwrap().assets[1].quantity = Decimal::from(1000);
        let a = analyze(s, 90 * DAY);
        assert!(!a.windows[1].qualified);
        assert!(
            !a.windows[1]
                .gates
                .iter()
                .find(|g| g.name == "Outlier independence")
                .unwrap()
                .passed
        );
    }
    #[test]
    fn unknown_closed_prehistory_does_not_poison_later_months_or_same_asset_episodes() {
        let mut s = consistent_sample();
        // Same asset as subsequent known rounds, with no historical USD prices.
        for (id, time, quantity, sol) in [
            ("old-buy", 10 * DAY, 10, -10),
            ("old-sell", 11 * DAY, -10, 12),
        ] {
            let mut r = tx(id, time, quantity, sol, true);
            r.transaction.as_mut().unwrap().assets[0].asset = "token0".into();
            s.records.push(r);
        }
        let a = analyze(s, 90 * DAY);
        assert_eq!(a.status, "qualified_60d", "{:?}", a.windows);
        assert!(a
            .windows
            .iter()
            .all(|w| w.episodes == 32 && w.realized_usd == Some(Decimal::from(43))));
        assert!(a
            .positions
            .iter()
            .find(|p| p.asset == "token0")
            .unwrap()
            .realized_usd
            .is_none());
    }
    #[test]
    fn unknown_opening_basis_or_current_sale_still_withholds_month_results() {
        let mut s = consistent_sample();
        s.records.extend([
            tx("old-unknown-buy", 10 * DAY, 10, -10, true),
            tx("current-sale", 89 * DAY, -10, 12, true),
        ]);
        s.prices.push(Price {
            asset: "SOL".into(),
            timestamp: 89 * DAY,
            usd: Decimal::ONE,
            granularity: "test".into(),
            source: "synthetic test only".into(),
        });
        let a = analyze(s, 90 * DAY);
        assert!(a.windows[1].realized_usd.is_none());
        assert!(!a.windows[1].qualified);
        assert_eq!(a.status, "incomplete");
        let mut s = consistent_sample();
        s.prices.retain(|p| p.timestamp != 71 * DAY + 30);
        let a = analyze(s, 90 * DAY);
        assert!(a.windows[1].realized_usd.is_none());
        assert!(
            !a.windows[1]
                .gates
                .iter()
                .find(|g| g.name == "Complete history")
                .unwrap()
                .passed
        );
    }
    #[test]
    fn open_losses_and_failed_fees_remove_an_apparent_winner() {
        let mut s = consistent_sample();
        s.records.push(tx("open-loss", 89 * DAY, 10, -100, true));
        s.prices.push(Price {
            asset: "SOL".into(),
            timestamp: 89 * DAY,
            usd: Decimal::ONE,
            granularity: "test".into(),
            source: "test".into(),
        });
        s.prices.push(Price {
            asset: "token".into(),
            timestamp: 90 * DAY,
            usd: Decimal::ONE,
            granularity: "test".into(),
            source: "test".into(),
        });
        s.balances.insert("token".into(), Decimal::from(10));
        let a = analyze(s, 90 * DAY);
        assert_eq!(a.windows[1].total_usd, Some(Decimal::from(-47)));
        assert!(!a.windows[1].qualified);
        let mut s = consistent_sample();
        let mut failed = tx("failed", 89 * DAY, 0, 0, false);
        let t = failed.transaction.as_mut().unwrap();
        t.succeeded = false;
        t.fee_quantity = Some(Decimal::from(50));
        s.records.push(failed);
        s.prices.push(Price {
            asset: "SOL".into(),
            timestamp: 89 * DAY,
            usd: Decimal::ONE,
            granularity: "test".into(),
            source: "test".into(),
        });
        let a = analyze(s, 90 * DAY);
        assert_eq!(a.windows[1].total_usd, Some(Decimal::from(-7)));
        assert!(!a.windows[1].qualified);
    }
    #[test]
    fn provisional_history_and_balance_mismatches_withhold_totals() {
        let mut s = consistent_sample();
        s.records
            .last_mut()
            .unwrap()
            .transaction
            .as_mut()
            .unwrap()
            .finalized = false;
        let a = analyze(s, 90 * DAY);
        assert!(a
            .windows
            .iter()
            .all(|w| w.total_usd.is_none() && !w.qualified));
        assert!(a.activity.iter().any(|e| !e.finalized));
        let mut s = consistent_sample();
        s.balances.insert("unseen".into(), Decimal::ONE);
        let a = analyze(s, 90 * DAY);
        assert!(!a.coverage.balances_reconciled);
        assert!(a.windows.iter().all(|w| w.total_usd.is_none()));
    }

    #[test]
    fn recent_burst_and_unresolved_quote_trades_do_not_establish_a_month() {
        let mut s = consistent_sample();
        s.records
            .retain(|r| !r.id.starts_with("age-") && !r.id.starts_with("30-"));
        let a = analyze(s, 90 * DAY);
        assert!(!a.windows[1].qualified);
        assert!(
            !a.windows[1]
                .gates
                .iter()
                .find(|g| g.name == "Record age")
                .unwrap()
                .passed
        );
        let mut s = consistent_sample();
        let mut r = tx("quote-trade", 89 * DAY, 1, -100, true);
        r.transaction.as_mut().unwrap().assets[0].asset =
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into();
        s.records.push(r);
        let a = analyze(s, 90 * DAY);
        assert!(a.unresolved_records > 0);
        assert!(a
            .windows
            .iter()
            .all(|w| w.total_usd.is_none() && !w.qualified));
    }
    #[test]
    fn withdrawing_inventory_cannot_hide_an_open_loss() {
        let mut s = consistent_sample();
        s.records.extend([
            tx("withdraw-buy", 88 * DAY, 10, -100, true),
            tx("withdraw-out", 89 * DAY, -10, 0, false),
        ]);
        for time in [88 * DAY, 89 * DAY] {
            s.prices.push(Price {
                asset: "SOL".into(),
                timestamp: time,
                usd: Decimal::ONE,
                granularity: "test".into(),
                source: "synthetic test only".into(),
            });
        }
        let a = analyze(s, 90 * DAY);
        assert!(a
            .windows
            .iter()
            .all(|w| w.total_usd.is_none() && !w.qualified));
        assert!(a.activity.iter().any(|e| e.kind == "transfer_out"));
    }
    #[test]
    fn a_profitable_program_account_never_becomes_a_trader_rank() {
        let mut s = consistent_sample();
        s.coverage.execution_account_verified = false;
        let a = analyze(s, 90 * DAY);
        assert_eq!(a.status, "incomplete");
        assert!(a.windows.iter().all(|w| !w.qualified));
    }
    #[test]
    fn bnb_public_history_stays_unqualified_and_equal_quote_amounts_are_not_native_wrapping() {
        let mut s = consistent_sample();
        s.candidate.chain = Chain::Bnb;
        s.coverage.history_complete = false;
        for r in &mut s.records {
            let t = r.transaction.as_mut().unwrap();
            t.fee_asset = "BNB".into();
            for d in &mut t.assets {
                if d.asset == "SOL" {
                    d.asset = "BNB".into();
                }
            }
        }
        for p in &mut s.prices {
            if p.asset == "SOL" {
                p.asset = "BNB".into();
            }
        }
        let a = analyze(s.clone(), 90 * DAY);
        assert_eq!(a.status, "incomplete");
        assert!(a
            .windows
            .iter()
            .all(|w| !w.qualified && w.total_usd.is_none()));
        s.coverage.history_complete = true;
        let mut r = tx("quote-conversion", 89 * DAY, 10, -10, true);
        let t = r.transaction.as_mut().unwrap();
        t.fee_asset = "BNB".into();
        t.assets[0].asset = "0x8ac76a51cc950d9822d68b83fe1ad97b32cd580d".into();
        t.assets[1].asset = "0x55d398326f99059ff775485246999027b3197955".into();
        s.records.push(r);
        let a = analyze(s, 90 * DAY);
        assert!(a.unresolved_records > 0);
        assert!(a
            .windows
            .iter()
            .all(|w| !w.qualified && w.total_usd.is_none()));
    }
}
