//! Read-time evidence presentation. Spot marks never enter historical accounting.
use super::{accounting, model::*};
use crate::model::TokenQuote;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Received balance observations override the historical ledger quantity.
/// Neither a missing price nor a zero USD mark says the token balance is zero.
pub fn position_quantity(position: &Position, coverage: &Coverage) -> Decimal {
    coverage
        .balance_observations
        .get(&position.asset)
        .map(|o| o.quantity)
        .or_else(|| position.valuation.as_ref().map(|v| v.quantity))
        .unwrap_or(position.quantity)
}

pub fn current_positions_count(analysis: &Analysis) -> usize {
    let chain = analysis.candidate.chain;
    let mut assets: BTreeSet<&str> = analysis
        .positions
        .iter()
        .filter(|p| {
            !quote(chain, &p.asset) && position_quantity(p, &analysis.coverage) > Decimal::ZERO
        })
        .map(|p| p.asset.as_str())
        .collect();
    // A received holding can exist before its acquisition history is parsed.
    for (asset, observation) in &analysis.coverage.balance_observations {
        if observation.quantity > Decimal::ZERO && !quote(chain, asset) {
            assets.insert(asset);
        }
    }
    assets.len()
}

pub fn retain_current_positions(analysis: &mut Analysis) {
    let chain = analysis.candidate.chain;
    let coverage = &analysis.coverage;
    analysis
        .positions
        .retain(|p| !quote(chain, &p.asset) && position_quantity(p, coverage) > Decimal::ZERO);
}

/// A visibility filter only: missing/stale marks do not establish zero value.
pub fn valued_positions_count(analysis: &Analysis, at: u64) -> usize {
    let mut quantities: BTreeMap<&str, Decimal> = analysis
        .positions
        .iter()
        .map(|p| (p.asset.as_str(), position_quantity(p, &analysis.coverage)))
        .collect();
    for (asset, observation) in &analysis.coverage.balance_observations {
        quantities.insert(asset, observation.quantity);
    }
    quantities
        .into_iter()
        .filter(|(asset, quantity)| {
            !quote(analysis.candidate.chain, asset)
                && *quantity > Decimal::ZERO
                && positive_market_value(asset, *quantity, &analysis.markets, at)
        })
        .count()
}

fn positive_market_value(
    asset: &str,
    quantity: Decimal,
    markets: &BTreeMap<String, TokenQuote>,
    at: u64,
) -> bool {
    markets
        .get(asset)
        .filter(|mark| at.saturating_sub(mark.observed_at) <= 900)
        .and_then(|mark| mark.price_usd)
        .filter(|price| *price > Decimal::ZERO)
        .and_then(|price| price.checked_mul(quantity.abs()))
        .is_some_and(|value| value > Decimal::ZERO)
}

pub fn retain_valued_summary_activity(analysis: &mut Analysis, at: u64) {
    let markets = &analysis.markets;
    let chain = analysis.candidate.chain;
    analysis.activity.retain(|activity| {
        !matches!(
            activity.kind.as_str(),
            "transfer_in" | "transfer_out" | "funding_in" | "funding_out"
        ) || activity
            .value_usd
            .is_some_and(|value| value != Decimal::ZERO)
            || activity
                .asset
                .as_ref()
                .zip(activity.quantity)
                .is_some_and(|(asset, quantity)| {
                    (quote(chain, asset) && quantity != Decimal::ZERO)
                        || positive_market_value(asset, quantity, markets, at)
                })
    });
}

/// Retain executions whose economics need inspection. Only hide complete,
/// transfer-only records when none of their movements has a received value.
fn unvalued_transfer(
    chain: crate::model::Chain,
    record: &Record,
    activities: &[&Activity],
    markets: &BTreeMap<String, TokenQuote>,
    at: u64,
) -> bool {
    let Some(tx) = &record.transaction else {
        return false;
    };
    tx.succeeded
        && tx.finalized
        && tx.movement_complete
        && !tx.swap_evidence
        && !tx.assets.is_empty()
        && !activities.iter().any(|a| {
            !matches!(
                a.kind.as_str(),
                "transfer_in" | "transfer_out" | "funding_in" | "funding_out"
            ) || a.value_usd.is_some_and(|value| value != Decimal::ZERO)
        })
        && !tx
            .assets
            .iter()
            .any(|movement| {
                (quote(chain, &movement.asset) && movement.quantity != Decimal::ZERO)
                    || positive_market_value(&movement.asset, movement.quantity, markets, at)
            })
}

pub fn enrich(
    analysis: &mut Analysis,
    snapshot: &Snapshot,
    markets: BTreeMap<String, TokenQuote>,
    at: u64,
) {
    if snapshot.coverage.balances_observed_at.is_some()
        || !snapshot.coverage.balance_observations.is_empty()
    {
        for (asset, quantity) in &snapshot.balances {
            if *quantity > Decimal::ZERO
                && !quote(snapshot.candidate.chain, asset)
                && !analysis.positions.iter().any(|p| p.asset == *asset)
            {
                analysis.positions.push(Position {
                    asset: asset.clone(),
                    quantity: Decimal::ZERO,
                    known_cost_usd: Decimal::ZERO,
                    basis_coverage: Decimal::ZERO,
                    market_value_usd: None,
                    realized_usd: None,
                    first_acquired_at: None,
                    last_activity_at: None,
                    valuation: None,
                    average_entry_usd: None,
                });
            }
        }
    }
    for position in &mut analysis.positions {
        let mark = markets.get(&position.asset);
        let observation = snapshot.coverage.balance_observations.get(&position.asset);
        let received = observation
            .map(|o| (o.quantity, o.observed_at))
            .or_else(|| {
                snapshot
                    .coverage
                    .balances_observed_at
                    .and_then(|time| snapshot.balances.get(&position.asset).map(|q| (*q, time)))
            });
        let (quantity, quantity_source, quantity_observed_at) = match received {
            Some((q, t)) => (q, "RPC token balance", Some(t)),
            None => (
                position.quantity,
                "Reconstructed from saved transactions",
                snapshot.coverage.last_collected_at,
            ),
        };
        if quantity != position.quantity
            || !analysis.coverage.history_complete
            || !analysis.coverage.balances_reconciled
        {
            position.average_entry_usd = None;
        }
        let fresh = mark.filter(|m| at.saturating_sub(m.observed_at) <= 900);
        let price = fresh.and_then(|m| m.price_usd);
        position.market_value_usd = if quantity == Decimal::ZERO {
            Some(Decimal::ZERO)
        } else {
            price.and_then(|p| p.checked_mul(quantity))
        };
        position.valuation = Some(PositionValuation {
            quantity,
            quantity_source: quantity_source.into(),
            quantity_observed_at,
            quantity_block: observation.map(|o| o.block.clone()),
            price_usd: price,
            price_observed_at: mark.map(|m| m.observed_at),
            source: mark.map(|m| m.source.clone()),
            detail: if quantity == Decimal::ZERO {
                "Zero quantity at the stated balance/history time.".into()
            } else if price.is_some() {
                "Quantity × current market mark. Read times are separate; incomplete history may make reconstructed quantity inaccurate. Liquidity and executable proceeds are unproved.".into()
            } else if mark.is_some_and(|m| at.saturating_sub(m.observed_at) > 900) {
                "Market mark is older than 15 minutes; refresh is pending. An old mark is not a current valuation.".into()
            } else {
                mark.map(|m| m.detail.clone()).unwrap_or(
                    "Market enrichment is pending; no current price has been received.".into(),
                )
            },
        });
    }
    analysis.markets = markets;
}

pub fn activity_page(
    snapshot: &Snapshot,
    markets: &BTreeMap<String, TokenQuote>,
    request: &ActivityRequest,
) -> Result<Value, String> {
    activity_page_with_revision(snapshot, markets, request, None)
}

pub fn activity_page_with_revision(
    snapshot: &Snapshot,
    markets: &BTreeMap<String, TokenQuote>,
    request: &ActivityRequest,
    saved_revision: Option<String>,
) -> Result<Value, String> {
    let mut records: Vec<_> = snapshot.records.iter().collect();
    records.sort_by_key(|r| {
        std::cmp::Reverse((
            record_block(r).unwrap_or(0),
            record_time(r).unwrap_or(0),
            r.transaction
                .as_ref()
                .and_then(|t| t.index)
                .unwrap_or(u64::MAX),
            r.id.clone(),
        ))
    });
    let base_revision = if let Some(revision) = saved_revision {
        revision
    } else {
        let mut hasher = Sha256::new();
        for record in &records {
            hasher.update(serde_json::to_vec(record).map_err(|e| e.to_string())?);
        }
        format!("{:x}", hasher.finalize())
    };
    let analysis = accounting::analyze(
        snapshot,
        snapshot
            .coverage
            .last_collected_at
            .unwrap_or_else(now)
            .saturating_add(1),
    );
    let mut activities: BTreeMap<&str, Vec<&Activity>> = BTreeMap::new();
    for event in &analysis.activity {
        activities.entry(&event.tx).or_default().push(event);
    }
    let saved_total = records.len();
    let at = now();
    // Direct source inspection always includes the requested record.
    if !request.include_unvalued && request.transaction.is_none() {
        records.retain(|record| {
            !unvalued_transfer(
                snapshot.candidate.chain,
                record,
                activities
                    .get(record.id.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                markets,
                at,
            )
        });
    }
    // Bind continuation to both the saved evidence and the filtered membership.
    // A mark refresh, expiry or filter toggle must never silently skip rows.
    let mut hasher = Sha256::new();
    hasher.update(base_revision.as_bytes());
    hasher.update([u8::from(request.include_unvalued)]);
    for record in &records {
        hasher.update((record.id.len() as u64).to_be_bytes());
        hasher.update(record.id.as_bytes());
    }
    let revision = format!("{:x}", hasher.finalize());
    let offset = if let Some(cursor) = &request.cursor {
        if cursor.len() > 512 {
            return Err("Invalid activity cursor.".into());
        }
        let decoded = URL_SAFE_NO_PAD
            .decode(cursor)
            .map_err(|_| "Invalid activity cursor.")?;
        let (saved, offset): (String, usize) =
            serde_json::from_slice(&decoded).map_err(|_| "Invalid activity cursor.")?;
        if saved != revision {
            return Err(
                "Activity changed; reload the first page to continue without skipped transactions."
                    .into(),
            );
        }
        offset
    } else {
        0
    };
    if offset > records.len() {
        return Err("Invalid activity offset.".into());
    }
    let row = |record: &&Record| {
        let tx = record.transaction.as_ref();
        json!({"tx":record.id,"timestamp":record_time(record),"outcome":match tx {None=>"awaiting_evidence",Some(t) if !t.succeeded=>"failed",Some(t) if !t.finalized=>"provisional",Some(t) if !t.movement_complete=>"incomplete",Some(_)=>"received"},
            "block":record_block(record),"index":tx.and_then(|t|t.index),"finalized":tx.map(|t|t.finalized),
            "movements":tx.map(|t|&t.assets),"fee_asset":tx.map(|t|&t.fee_asset),"fee_quantity":tx.and_then(|t|t.fee_quantity),
            "swap_evidence":tx.map(|t|t.swap_evidence),"movement_complete":tx.map(|t|t.movement_complete),
            "counterparties":tx.map(|t|&t.counterparties),"notes":tx.map(|t|&t.notes),"error":record.error,
            "activities":activities.get(record.id.as_str()).cloned().unwrap_or_default(),"provider":snapshot.coverage.provider,
            "raw":if request.transaction.is_some(){Some(&record.raw)}else{None}})
    };
    if let Some(id) = &request.transaction {
        let record = records
            .iter()
            .find(|r| r.id == *id)
            .ok_or("Transaction has not been saved for this wallet.")?;
        return Ok(json!({"transaction":row(record),"markets":markets,"revision":revision}));
    }
    let limit = request.limit.unwrap_or(25).clamp(1, 100);
    let end = (offset + limit).min(records.len());
    let next = (end < records.len())
        .then(|| URL_SAFE_NO_PAD.encode(serde_json::to_vec(&(revision.clone(), end)).unwrap()));
    Ok(
        json!({"transactions":records[offset..end].iter().map(row).collect::<Vec<_>>(),"markets":markets,"next_cursor":next,"total":records.len(),"saved_total":saved_total,"hidden_count":saved_total-records.len(),"include_unvalued":request.include_unvalued,"revision":revision,"scope":"Complete transfer-only records without a received positive value are hidden by default. Trades, failed, provisional and unresolved executions remain visible. All source records remain inspectable; source-history completeness is separate."}),
    )
}
fn record_block(record: &Record) -> Option<u64> {
    record
        .transaction
        .as_ref()
        .map(|t| t.block)
        .or_else(|| record.raw["slot"].as_u64())
        .or_else(|| record.raw["block_number"].as_u64())
}
fn record_time(record: &Record) -> Option<u64> {
    record
        .transaction
        .as_ref()
        .map(|t| t.timestamp)
        .or_else(|| record.raw["blockTime"].as_u64())
        .or_else(|| {
            record.raw["timestamp"]
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .and_then(|d| u64::try_from(d.timestamp()).ok())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Chain;
    fn snapshot() -> Snapshot {
        Snapshot {
            candidate: Candidate {
                chain: Chain::Solana,
                wallet: "21rgbFW6sujQovCw3qt6R2EdE97Yzzvk8sSc37Bb72Cm".into(),
                discovered_at: 1,
                sources: vec![],
                observed_tokens: vec![],
            },
            coverage: Coverage {
                last_collected_at: Some(10000),
                ..Default::default()
            },
            records: vec![],
            prices: vec![],
            balances: BTreeMap::new(),
        }
    }
    fn request() -> ActivityRequest {
        ActivityRequest {
            chain: Chain::Solana,
            wallet: snapshot().candidate.wallet,
            cursor: None,
            limit: Some(25),
            transaction: None,
            include_unvalued: false,
        }
    }

    fn position(asset: &str, quantity: Decimal) -> Position {
        Position {
            asset: asset.into(),
            quantity,
            known_cost_usd: Decimal::ZERO,
            basis_coverage: Decimal::ZERO,
            market_value_usd: None,
            realized_usd: None,
            first_acquired_at: None,
            last_activity_at: None,
            valuation: None,
            average_entry_usd: None,
        }
    }

    #[test]
    fn holdings_use_received_balances_and_do_not_confuse_missing_or_zero_prices_with_zero_tokens() {
        for chain in [
            crate::model::Chain::Solana,
            crate::model::Chain::Robinhood,
            crate::model::Chain::Bnb,
        ] {
            let mut s = snapshot();
            s.candidate.chain = chain;
            for (asset, quantity) in [
                ("received-zero", Decimal::ZERO),
                ("received-positive", 4.into()),
                ("rpc-only", 2.into()),
                (native(chain), 10.into()),
            ] {
                s.balances.insert(asset.into(), quantity);
                s.coverage.balance_observations.insert(
                    asset.into(),
                    BalanceObservation {
                        quantity,
                        observed_at: 9900,
                        block: "fixture block".into(),
                        source: "fixture balance".into(),
                    },
                );
            }
            let mut a = accounting::analyze(&s, 10000);
            a.positions = vec![
                position("closed", Decimal::ZERO),
                position("received-zero", 100.into()),
                position("received-positive", Decimal::ZERO),
                position("unpriced", 3.into()),
                position("zero-usd-mark", 5.into()),
                position("tiny", Decimal::new(1, 28)),
            ];
            let markets = BTreeMap::from([(
                "zero-usd-mark".into(),
                TokenQuote {
                    asset: "zero-usd-mark".into(),
                    price_usd: Some(Decimal::ZERO),
                    observed_at: 10000,
                    source: "fixture zero price".into(),
                    ..Default::default()
                },
            )]);
            // Cached summaries also count the RPC-only holding before enrich.
            assert_eq!(current_positions_count(&a), 5);
            enrich(&mut a, &s, markets, 10000);
            retain_current_positions(&mut a);
            let assets: BTreeSet<_> = a.positions.iter().map(|p| p.asset.as_str()).collect();
            assert_eq!(
                assets,
                BTreeSet::from([
                    "received-positive",
                    "rpc-only",
                    "unpriced",
                    "zero-usd-mark",
                    "tiny"
                ])
            );
            assert_eq!(current_positions_count(&a), a.positions.len());
            assert_eq!(
                a.positions
                    .iter()
                    .find(|p| p.asset == "unpriced")
                    .unwrap()
                    .market_value_usd,
                None
            );
            assert_eq!(
                a.positions
                    .iter()
                    .find(|p| p.asset == "zero-usd-mark")
                    .unwrap()
                    .market_value_usd,
                Some(Decimal::ZERO)
            );
            assert!(a
                .positions
                .iter()
                .all(|p| position_quantity(p, &a.coverage) > Decimal::ZERO));
        }
    }

    #[test]
    fn removing_closed_position_rows_preserves_the_realized_loss_and_trade_history() {
        let mut s = snapshot();
        for (id, time, tokens, sol) in [("buy", 9000, 10, -2), ("sell", 9100, -10, 1)] {
            s.records.push(Record {
                id: id.into(),
                raw: json!({"fixture":true}),
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
                            asset: "closed-token".into(),
                            quantity: tokens.into(),
                        },
                        Delta {
                            asset: "SOL".into(),
                            quantity: sol.into(),
                        },
                    ],
                    fee_asset: "SOL".into(),
                    fee_quantity: Some(Decimal::ZERO),
                    movement_complete: true,
                    swap_evidence: true,
                    notes: vec![],
                    counterparties: vec![],
                }),
            });
            s.prices.push(Price {
                asset: "SOL".into(),
                timestamp: time,
                usd: 10.into(),
                granularity: "minute".into(),
                source: "fixture historical price".into(),
            });
        }
        let mut a = accounting::analyze(&s, 10000);
        assert_eq!(a.positions[0].quantity, Decimal::ZERO);
        assert_eq!(a.positions[0].realized_usd, Some(Decimal::from(-10)));
        let windows = serde_json::to_value(&a.windows).unwrap();
        let activity = serde_json::to_value(&a.activity).unwrap();
        let status = a.status.clone();
        enrich(&mut a, &s, BTreeMap::new(), 10000);
        retain_current_positions(&mut a);
        assert!(a.positions.is_empty());
        assert_eq!(current_positions_count(&a), 0);
        assert_eq!(serde_json::to_value(&a.windows).unwrap(), windows);
        assert_eq!(serde_json::to_value(&a.activity).unwrap(), activity);
        assert_eq!(a.status, status);
        assert_eq!(a.records, 2);
        assert_eq!(s.records.len(), 2);
    }
    #[test]
    fn valued_position_counts_use_current_balance_and_fresh_marks_on_all_chains() {
        let at = 10000;
        for chain in [
            crate::model::Chain::Solana,
            crate::model::Chain::Robinhood,
            crate::model::Chain::Bnb,
        ] {
            let mut snapshot = snapshot();
            snapshot.candidate.chain = chain;
            let mut analysis = accounting::analyze(&snapshot, at);
            analysis.positions = vec![
                position("positive", 10.into()),
                position("zero", 2.into()),
                position("unpriced", 3.into()),
                position("stale", 4.into()),
                position("tiny", Decimal::new(1, 28)),
            ];
            for (asset, price, observed_at) in [
                ("positive", 2.into(), at),
                ("zero", Decimal::ZERO, at),
                ("stale", 2.into(), at - 901),
                ("tiny", 1.into(), at),
                ("rpc-only", 1.into(), at),
            ] {
                analysis.markets.insert(
                    asset.into(),
                    TokenQuote {
                        asset: asset.into(),
                        price_usd: Some(price),
                        observed_at,
                        ..Default::default()
                    },
                );
            }
            assert_eq!(valued_positions_count(&analysis, at), 2);
            analysis.coverage.balance_observations.insert(
                "positive".into(),
                BalanceObservation {
                    quantity: Decimal::ZERO,
                    observed_at: at,
                    block: "fixture".into(),
                    source: "fixture".into(),
                },
            );
            analysis.coverage.balance_observations.insert(
                "rpc-only".into(),
                BalanceObservation {
                    quantity: 2.into(),
                    observed_at: at,
                    block: "fixture".into(),
                    source: "fixture".into(),
                },
            );
            assert_eq!(valued_positions_count(&analysis, at), 2);
            assert_eq!(valued_positions_count(&analysis, at + 901), 0);
        }
    }

    #[test]
    fn transfer_filter_pages_useful_executions_and_preserve_all_source_records() {
        let mut s = snapshot();
        let tx = |id: &str, asset: &str| Transaction {
            id: id.into(),
            timestamp: 1000,
            block: 1000,
            index: None,
            finalized: true,
            succeeded: true,
            assets: vec![Delta {
                asset: asset.into(),
                quantity: 10.into(),
            }],
            fee_asset: "SOL".into(),
            fee_quantity: Some(Decimal::ZERO),
            movement_complete: true,
            swap_evidence: false,
            notes: vec![],
            counterparties: vec![],
        };
        for n in 0..30 {
            let id = format!("unpriced-{n:02}");
            s.records.push(Record {
                id: id.clone(),
                raw: json!({"received":"fixture"}),
                transaction: Some(tx(&id, "unpriced")),
                error: None,
            });
        }
        for (id, mut transaction) in [
            ("priced", tx("priced", "priced")),
            ("trade", tx("trade", "unpriced")),
            ("failed", tx("failed", "unpriced")),
            ("incomplete", tx("incomplete", "unpriced")),
            ("provisional", tx("provisional", "unpriced")),
            ("fee-only", tx("fee-only", "unpriced")),
            ("funding", tx("funding", "SOL")),
        ] {
            match id {
                "trade" => transaction.swap_evidence = true,
                "failed" => transaction.succeeded = false,
                "incomplete" => transaction.movement_complete = false,
                "provisional" => transaction.finalized = false,
                "fee-only" => transaction.assets.clear(),
                _ => (),
            }
            s.records.push(Record {
                id: id.into(),
                raw: json!({"received":"fixture"}),
                transaction: Some(transaction),
                error: None,
            });
        }
        s.records.push(Record {
            id: "pending".into(),
            raw: json!({}),
            transaction: None,
            error: Some("pending".into()),
        });
        let at = now();
        let mut markets = BTreeMap::from([(
            "priced".into(),
            TokenQuote {
                asset: "priced".into(),
                price_usd: Some(1.into()),
                observed_at: at,
                ..Default::default()
            },
        )]);
        let original = serde_json::to_value(&s).unwrap();
        let accounting_before = serde_json::to_value(accounting::analyze(&s, 1001)).unwrap();
        let mut req = request();
        req.limit = Some(2);
        let page = activity_page(&s, &markets, &req).unwrap();
        assert_eq!(page["total"], 8);
        assert_eq!(page["saved_total"], 38);
        assert_eq!(page["hidden_count"], 30);
        let first_cursor = page["next_cursor"].as_str().unwrap().to_owned();
        let mut ids = BTreeSet::new();
        loop {
            let page = activity_page(&s, &markets, &req).unwrap();
            for row in page["transactions"].as_array().unwrap() {
                assert!(ids.insert(row["tx"].as_str().unwrap().to_owned()));
            }
            req.cursor = page["next_cursor"].as_str().map(str::to_owned);
            if req.cursor.is_none() {
                break;
            }
        }
        assert_eq!(
            ids,
            BTreeSet::from(
                [
                    "priced",
                    "trade",
                    "failed",
                    "incomplete",
                    "provisional",
                    "fee-only",
                    "funding",
                    "pending"
                ]
                .map(str::to_owned)
            )
        );
        req.cursor = Some(first_cursor.clone());
        req.include_unvalued = true;
        assert!(activity_page(&s, &markets, &req)
            .unwrap_err()
            .starts_with("Activity changed;"));
        req.cursor = None;
        req.limit = Some(100);
        assert_eq!(
            activity_page(&s, &markets, &req).unwrap()["transactions"]
                .as_array()
                .unwrap()
                .len(),
            38
        );
        req.include_unvalued = false;
        req.transaction = Some("unpriced-00".into());
        assert_eq!(
            activity_page(&s, &markets, &req).unwrap()["transaction"]["raw"],
            json!({"received":"fixture"})
        );
        req.transaction = None;
        req.cursor = Some(first_cursor);
        markets.insert(
            "unpriced".into(),
            TokenQuote {
                asset: "unpriced".into(),
                price_usd: Some(1.into()),
                observed_at: at,
                ..Default::default()
            },
        );
        assert!(activity_page(&s, &markets, &req)
            .unwrap_err()
            .starts_with("Activity changed;"));
        assert_eq!(serde_json::to_value(&s).unwrap(), original);
        assert_eq!(
            serde_json::to_value(accounting::analyze(&s, 1001)).unwrap(),
            accounting_before
        );
    }

    #[test]
    fn every_pending_record_is_pageable_and_changed_evidence_rejects_old_cursors() {
        let mut s = snapshot();
        for n in 0..121 {
            s.records.push(Record {
                id: format!("tx{n:03}"),
                raw: json!({"blockTime":n+1}),
                transaction: None,
                error: Some("Receipt pending".into()),
            });
        }
        let mut req = request();
        let markets = BTreeMap::new();
        let mut ids = std::collections::BTreeSet::new();
        let mut first_cursor = None;
        loop {
            let page = activity_page(&s, &markets, &req).unwrap();
            for row in page["transactions"].as_array().unwrap() {
                assert_eq!(row["outcome"], "awaiting_evidence");
                assert!(row["movements"].is_null());
                assert_eq!(row["error"], "Receipt pending");
                assert!(ids.insert(row["tx"].as_str().unwrap().to_string()));
            }
            req.cursor = page["next_cursor"].as_str().map(str::to_owned);
            if first_cursor.is_none() {
                first_cursor = req.cursor.clone();
            }
            if req.cursor.is_none() {
                break;
            }
        }
        assert_eq!(ids.len(), 121);
        req.cursor = first_cursor;
        s.records[0].error = Some("Received error changed".into());
        assert!(activity_page(&s, &markets, &req)
            .unwrap_err()
            .starts_with("Activity changed;"));
        req.cursor = Some("a".repeat(513));
        assert!(activity_page(&s, &markets, &req).is_err());
        req.cursor = None;
        req.transaction = Some("tx000".into());
        assert_eq!(
            activity_page(&s, &markets, &req).unwrap()["transaction"]["raw"]["blockTime"],
            1
        );
    }
    #[test]
    fn current_marks_never_supply_historical_entry_prices_or_qualification() {
        let mut s = snapshot();
        s.records.push(Record {
            id: "buy".into(),
            raw: json!({"synthetic":true}),
            error: None,
            transaction: Some(Transaction {
                id: "buy".into(),
                timestamp: 9000,
                block: 1,
                index: Some(0),
                finalized: true,
                succeeded: true,
                assets: vec![
                    Delta {
                        asset: "token".into(),
                        quantity: Decimal::from(100),
                    },
                    Delta {
                        asset: "SOL".into(),
                        quantity: Decimal::from(-2),
                    },
                ],
                fee_asset: "SOL".into(),
                fee_quantity: Some(Decimal::new(5, 5)),
                movement_complete: true,
                swap_evidence: true,
                notes: vec![],
                counterparties: vec![],
            }),
        });
        let mut a = accounting::analyze(&s, 10000);
        let markets = BTreeMap::from([(
            "token".into(),
            TokenQuote {
                asset: "token".into(),
                name: Some("Synthetic token".into()),
                symbol: Some("TEST".into()),
                price_usd: Some(Decimal::from(7)),
                observed_at: 10000,
                source: "Synthetic current mark".into(),
                ..Default::default()
            },
        )]);
        enrich(&mut a, &s, markets.clone(), 10000);
        assert_eq!(a.positions[0].market_value_usd, Some(Decimal::from(700)));
        assert_eq!(
            a.activity[0].pricing.unit_price_quote,
            Some(Decimal::new(2, 2))
        );
        assert_eq!(a.activity[0].pricing.unit_price_usd, None);
        assert_eq!(a.activity[0].pricing.fee_quantity, Some(Decimal::new(5, 5)));
        assert_eq!(a.activity[0].pricing.fee_usd, None);
        assert!(!a.windows.iter().any(|w| w.qualified));
        s.prices.push(Price {
            asset: "SOL".into(),
            timestamp: 9000,
            usd: Decimal::from(100),
            source: "Synthetic historical candle".into(),
            granularity: "hour".into(),
        });
        let priced = accounting::analyze(&s, 10000);
        assert_eq!(
            priced.activity[0].pricing.unit_price_usd,
            Some(Decimal::from(2))
        );
        assert_eq!(priced.activity[0].pricing.fee_usd, Some(Decimal::new(5, 3)));
        assert_eq!(
            priced.activity[0]
                .pricing
                .quote_conversion
                .as_ref()
                .unwrap()
                .source,
            "Synthetic historical candle"
        );
        enrich(&mut a, &s, markets, 10901);
        assert_eq!(a.positions[0].market_value_usd, None);
        assert!(a.positions[0]
            .valuation
            .as_ref()
            .unwrap()
            .detail
            .contains("older than 15 minutes"));
    }
    #[test]
    fn received_balance_and_zero_are_distinct_from_reconstructed_quantity_and_missing_price() {
        let mut s = snapshot();
        s.coverage.balances_observed_at = Some(9900);
        s.balances.insert("token".into(), Decimal::from(4));
        let mut a = accounting::analyze(&s, 10000);
        a.positions.push(Position {
            asset: "token".into(),
            quantity: Decimal::from(100),
            known_cost_usd: Decimal::ZERO,
            basis_coverage: Decimal::ZERO,
            market_value_usd: None,
            realized_usd: None,
            first_acquired_at: None,
            last_activity_at: None,
            valuation: None,
            average_entry_usd: None,
        });
        let markets = BTreeMap::from([(
            "token".into(),
            TokenQuote {
                asset: "token".into(),
                price_usd: Some(Decimal::from(7)),
                observed_at: 10000,
                source: "Synthetic mark".into(),
                ..Default::default()
            },
        )]);
        enrich(&mut a, &s, markets, 10000);
        assert_eq!(a.positions[0].quantity, Decimal::from(100));
        assert_eq!(a.positions[0].market_value_usd, Some(Decimal::from(28)));
        assert_eq!(
            a.positions[0].valuation.as_ref().unwrap().quantity,
            Decimal::from(4)
        );
        assert_eq!(
            a.positions[0]
                .valuation
                .as_ref()
                .unwrap()
                .quantity_observed_at,
            Some(9900)
        );
        s.balances.insert("token".into(), Decimal::ZERO);
        enrich(&mut a, &s, BTreeMap::new(), 10000);
        assert_eq!(a.positions[0].market_value_usd, Some(Decimal::ZERO));
        s.balances.remove("token");
        enrich(&mut a, &s, BTreeMap::new(), 10000);
        assert_eq!(a.positions[0].market_value_usd, None);
    }
}
