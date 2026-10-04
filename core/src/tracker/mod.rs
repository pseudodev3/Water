mod accounting;
mod bnb;
pub mod model;
mod providers;
mod store;
mod venues;

use crate::{config::Config, model::Chain, providers::gecko::GeckoClient};
use model::*;
use providers::Providers;
use serde_json::{json, Value};
use std::{collections::BTreeSet, sync::Arc};
use store::Store;

pub struct Tracker {
    store: Option<Arc<Store>>,
    providers: Option<Providers>,
    gecko: GeckoClient,
    error: Option<String>,
    interval: u64,
    cohort_limit: usize,
    record_limit: usize,
    public_nominations: bool,
}

fn env_number(name: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
        .clamp(min, max)
}
fn secret(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn helius_keys(list: Option<String>, legacy: Option<String>) -> Vec<String> {
    let mut keys = Vec::new();
    for key in list
        .or(legacy)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .take(8)
    {
        if !keys.iter().any(|v| v == key) {
            keys.push(key.to_string());
        }
    }
    keys
}

fn balanced_candidates(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut groups: std::collections::BTreeMap<&str, std::collections::VecDeque<Candidate>> =
        std::collections::BTreeMap::new();
    for c in candidates {
        groups.entry(c.chain.key()).or_default().push_back(c);
    }
    let mut out = Vec::new();
    while groups.values().any(|g| !g.is_empty()) {
        for g in groups.values_mut() {
            if let Some(c) = g.pop_front() {
                out.push(c);
            }
        }
    }
    out
}

#[cfg(test)]
mod config_tests {
    use super::*;
    #[test]
    fn credential_lists_are_trimmed_deduplicated_and_do_not_change_budget() {
        assert_eq!(
            helius_keys(Some(" a, b, a, , c ".into()), Some("legacy".into())),
            vec!["a", "b", "c"]
        );
        assert_eq!(helius_keys(None, Some(" old ".into())), vec!["old"]);
        assert!(helius_keys(None, None).is_empty());
    }
    #[test]
    fn discovery_does_not_fill_the_cohort_before_a_chain_gets_a_turn() {
        let candidate = |chain, w: &str| Candidate {
            observed_tokens: Vec::new(),
            chain,
            wallet: w.into(),
            discovered_at: 0,
            sources: vec![],
        };
        let candidates = vec![
            candidate(Chain::Solana, "s1"),
            candidate(Chain::Solana, "s2"),
            candidate(Chain::Robinhood, "r1"),
            candidate(Chain::Bnb, "b1"),
        ];
        let first = balanced_candidates(candidates)
            .into_iter()
            .take(3)
            .map(|c| c.chain.key())
            .collect::<BTreeSet<_>>();
        assert_eq!(first, BTreeSet::from(["bnb", "robinhood", "solana"]));
    }
}

fn demote_stale(analysis: &mut Analysis) {
    if analysis
        .coverage
        .last_collected_at
        .is_some_and(|t| now().saturating_sub(t) > 3600)
    {
        analysis.status = "stale".into();
        for window in &mut analysis.windows {
            window.qualified = false;
            for gate in &mut window.gates {
                if gate.name == "Recent and fresh" {
                    gate.passed = false;
                }
            }
        }
    }
}

fn ranking_key(
    analysis: &Analysis,
) -> (
    Option<rust_decimal::Decimal>,
    Option<rust_decimal::Decimal>,
    usize,
) {
    let windows: Vec<_> = match analysis.status.as_str() {
        "qualified_60d" => analysis.windows.iter().collect(),
        "qualified_30d" => analysis.windows.last().into_iter().collect(),
        _ => vec![],
    };
    (
        windows.iter().filter_map(|w| w.profit_factor).min(),
        windows
            .iter()
            .filter_map(|w| w.profit_without_largest_usd)
            .min(),
        windows.iter().map(|w| w.episodes).min().unwrap_or(0),
    )
}

impl Tracker {
    pub fn new(http: reqwest::Client, config: Config, gecko: GeckoClient) -> Arc<Self> {
        let (store, error) = match secret("WATER_TRACKER_DB_PATH") {
            Some(path) => match Store::open(&path) {
                Ok(store) => (Some(Arc::new(store)), None),
                Err(e) => (None, Some(e)),
            },
            None => (
                None,
                Some(
                    "Wallet collection is paused because evidence storage is not configured."
                        .into(),
                ),
            ),
        };
        let providers = store.as_ref().map(|store| Providers {
            http,
            config,
            store: store.clone(),
            helius_keys: helius_keys(secret("HELIUS_API_KEYS"), secret("HELIUS_API_KEY")),
            helius_credit_limit: env_number("WATER_TRACKER_HELIUS_CREDITS_31D", 800000, 10, 1000000)
                as u64,
            fomo_key: secret("FOMO_DISCOVERY_API_KEY"),
            rh_trace_url: std::env::var("ROBINHOOD_TRACE_RPC_URL")
                .unwrap_or_else(|_| "https://robinhood.drpc.org".into()),
            bnb_trace_url: std::env::var("BNB_TRACE_RPC_URL")
                .unwrap_or_else(|_| "https://bsc.drpc.org".into()),
            daily_limit: env_number("WATER_TRACKER_DAILY_REQUESTS", 2000, 10, 2500) as u64,
        });
        Arc::new(Self {
            store,
            providers,
            gecko,
            error,
            interval: env_number("WATER_TRACKER_INTERVAL_SECONDS", 60, 30, 3600) as u64,
            cohort_limit: env_number("WATER_TRACKER_COHORT_LIMIT", 12, 2, 48),
            record_limit: env_number("WATER_TRACKER_RECORD_LIMIT", 10000, 100, 50000),
            public_nominations: std::env::var("WATER_TRACKER_ALLOW_PUBLIC_NOMINATIONS").as_deref()
                == Ok("true"),
        })
    }

    pub fn start(self: &Arc<Self>) {
        if self.store.is_none() {
            return;
        }
        let tracker = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = tracker.tick().await {
                    if let Some(store) = &tracker.store {
                        let _ = store.set_state("collector_error", &error);
                    }
                    tracing::warn!("Wallet collector: {}", error);
                }
                tokio::time::sleep(std::time::Duration::from_secs(tracker.interval)).await;
            }
        });
    }

    pub fn status(&self) -> Value {
        let Some(store) = &self.store else {
            return json!({"enabled":false,"detail":self.error,"wallets":[],"policy":POLICY});
        };
        let providers = self.providers.as_ref().unwrap();
        let used = store
            .state(&format!("requests:{}", now() / DAY))
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        json!({"enabled":true,"nomination_enabled":self.public_nominations,"detail":store.state("collector_error").ok().flatten(),"policy":POLICY,"interval_seconds":self.interval,"cohort_limit":self.cohort_limit,"requests_today":used,"daily_request_limit":providers.daily_limit,"solana_indexed_access":!providers.helius_keys.is_empty(),"rh_indexed_access":providers.config.blockscout_api_key.is_some(),"helius_key_count":providers.helius_keys.len(),"helius_credits_reserved_31d":store.helius_credits(now()).unwrap_or(0),"helius_credit_limit_31d":providers.helius_credit_limit,"bnb_history_scope":"Public token-transfer discovery; complete wallet/native-history coverage is unproved.","fomo_discovery_access":providers.fomo_key.is_some(),"last_discovery_at":store.state("discovery_time").ok().flatten().and_then(|v|v.parse::<u64>().ok()),"discovery_notes":store.state("discovery_notes").ok().flatten().and_then(|v|serde_json::from_str::<Value>(&v).ok()),"storage_configured":!store.path.is_empty(),"storage_durability":"Requires a persistent deployment volume; path configuration does not prove durability."})
    }

    pub fn list(&self) -> Result<Value, String> {
        let Some(store) = &self.store else {
            return Ok(json!({"status":self.status(),"wallets":[]}));
        };
        let mut analyses = store.analyses()?;
        for analysis in &mut analyses {
            demote_stale(analysis);
        }
        analyses.sort_by(|a, b| {
            let tier = |s: &str| match s {
                "qualified_60d" => 0,
                "qualified_30d" => 1,
                "observed" => 2,
                _ => 3,
            };
            tier(&a.status)
                .cmp(&tier(&b.status))
                .then_with(|| ranking_key(b).cmp(&ranking_key(a)))
                .then_with(|| a.candidate.chain.key().cmp(b.candidate.chain.key()))
                .then_with(|| a.candidate.wallet.cmp(&b.candidate.wallet))
        });
        let summaries: Vec<_> = analyses
            .into_iter()
            .map(|mut a| {
                a.activity.truncate(20);
                let mut value = serde_json::to_value(a).unwrap();
                let fields = value.as_object_mut().unwrap();
                fields.remove("positions");
                fields.remove("notes");
                value
            })
            .collect();
        Ok(
            json!({"status":self.status(),"scope":"ranking_summary; full positions and notes are in wallet detail","wallets":summaries}),
        )
    }

    pub fn detail(&self, chain: Chain, wallet: &str) -> Result<Option<Analysis>, String> {
        let wallet = wallet_key(chain, wallet)?;
        let Some(store) = &self.store else {
            return Ok(None);
        };
        let Some(snapshot) = store.snapshot(chain, &wallet)? else {
            return Ok(None);
        };
        let end = snapshot.coverage.last_collected_at.unwrap_or_else(now);
        let mut analysis = accounting::analyze(snapshot, end);
        demote_stale(&mut analysis);
        Ok(Some(analysis))
    }

    pub fn nominate(&self, request: WalletRequest) -> Result<Analysis, String> {
        if !self.public_nominations {
            return Err(
                "Public nominations are disabled to protect the shared free collection budget."
                    .into(),
            );
        }
        let wallet = wallet_key(request.chain, &request.wallet)?;
        let store = self
            .store
            .as_ref()
            .ok_or("Wallet collection is paused until evidence storage is configured.")?;
        let candidate=Candidate{observed_tokens:Vec::new(),chain:request.chain,wallet:wallet.clone(),discovered_at:now(),sources:vec![Source{name:"Added for research".into(),observed_at:now(),detail:"A nomination requests evidence collection; it does not verify ownership or profitability.".into(),profile:None}]};
        store.nominate(candidate, self.cohort_limit)?;
        let analysis = accounting::analyze(
            store
                .snapshot(request.chain, &wallet)?
                .ok_or("Wallet nomination was not saved.")?,
            now(),
        );
        store.save_analysis(&analysis)?;
        Ok(analysis)
    }

    pub fn token_overlap(&self, chain: Chain, token: &str) -> Result<Value, String> {
        let token = wallet_key(chain, token)?;
        let Some(store) = &self.store else {
            return Ok(json!({"wallets":[]}));
        };
        let mut wallets = Vec::new();
        for analysis in store.analyses()? {
            if analysis.candidate.chain.key() != chain.key()
                || !analysis.status.starts_with("qualified_")
                || analysis
                    .coverage
                    .last_collected_at
                    .is_none_or(|t| now().saturating_sub(t) > 3600)
            {
                continue;
            }
            if let Some(position) = analysis
                .positions
                .iter()
                .find(|p| p.asset == token && p.quantity > rust_decimal::Decimal::ZERO)
            {
                wallets.push(json!({"wallet":analysis.candidate.wallet,"status":analysis.status,"position":position,"analyzed_at":analysis.analyzed_at}));
            }
        }
        Ok(
            json!({"chain":chain,"token":token,"wallets":wallets,"scope":"Fresh qualifying wallets in Water's bounded collected cohort. Shared holdings do not prove independent ownership."}),
        )
    }

    async fn snapshot(&self, chain: Chain, wallet: &str) -> Result<Snapshot, String> {
        let store = self.store.clone().ok_or("Tracker is not configured.")?;
        let wallet = wallet.to_string();
        tokio::task::spawn_blocking(move || {
            store
                .snapshot(chain, &wallet)?
                .ok_or("Scheduled wallet disappeared.".into())
        })
        .await
        .map_err(|_| "Wallet evidence worker failed.")?
    }

    async fn update_analysis(&self, chain: Chain, wallet: &str, end: u64) -> Result<(), String> {
        let snapshot = self.snapshot(chain, wallet).await?;
        let end = snapshot.coverage.last_collected_at.unwrap_or(end);
        let store = self.store.clone().unwrap();
        tokio::task::spawn_blocking(move || {
            store.save_analysis(&accounting::analyze(snapshot, end))
        })
        .await
        .map_err(|_| "Wallet accounting worker failed.")?
    }

    async fn tick(&self) -> Result<(), String> {
        let store = self.store.as_ref().ok_or("Tracker is not configured.")?;
        let providers = self.providers.as_ref().unwrap();
        if store
            .state("discovery_time")?
            .and_then(|v| v.parse::<u64>().ok())
            .is_none_or(|t| now().saturating_sub(t) > 3600)
        {
            let (candidates, notes) = providers.discover(self.cohort_limit).await;
            for candidate in balanced_candidates(candidates) {
                let chain = candidate.chain;
                let wallet = candidate.wallet.clone();
                if store.nominate(candidate, self.cohort_limit).is_ok() {
                    self.update_analysis(chain, &wallet, now()).await?;
                }
            }
            store.set_state("discovery_time", &now().to_string())?;
            store.set_state("discovery_notes", &serde_json::to_string(&notes).unwrap())?;
        }
        let Some(candidate) = store.next_wallet(now())? else {
            return Ok(());
        };
        let snapshot = self.snapshot(candidate.chain, &candidate.wallet).await?;
        if snapshot.records.len() >= self.record_limit {
            let mut coverage = snapshot.coverage.clone();
            coverage.history_complete = false;
            coverage.notes=vec!["Wallet history reached its record budget. Qualification is paused; no missing history is treated as complete.".into()];
            store.save_page(&candidate, &[], &coverage, &snapshot.balances)?;
            self.update_analysis(candidate.chain, &candidate.wallet, now())
                .await?;
            return Ok(());
        }
        let known: BTreeSet<_> = snapshot.records.iter().map(|r| r.id.clone()).collect();
        let mut coverage = snapshot.coverage.clone();
        let mut records = Vec::new();
        let head = providers.page(&candidate, None).await?;
        let overlap = if matches!(candidate.chain, Chain::Bnb) {
            providers::range_covered(&head, &coverage.public_scan_ranges)
        } else {
            providers::head_covered(&head, &known)
        };
        coverage.provider = if matches!(candidate.chain, Chain::Solana) {
            if head.indexed {
                "Helius indexed Solana history"
            } else {
                "Public Solana signature history"
            }
        } else if matches!(candidate.chain, Chain::Bnb) {
            "BNB public token-transfer references and direct RPC/trace evidence"
        } else {
            "RH Blockscout and direct RPC/trace evidence"
        }
        .into();
        coverage.pages += 1;
        coverage.notes = head.notes.clone();
        if !coverage.backfill_started {
            coverage.cursor = head.cursor.clone();
            coverage.backfill_started = true;
            coverage.backfill_done = head.exhausted;
            coverage.head_complete = true;
        } else if !overlap && !head.exhausted {
            coverage.head_complete = false;
            if let Some(cursor) = head.cursor.clone() {
                if coverage.head_cursor.is_none() {
                    coverage.head_cursor = Some(cursor);
                } else if coverage.head_cursor.as_ref() != Some(&cursor)
                    && !coverage.head_cursors.contains(&cursor)
                {
                    coverage.head_cursors.push(cursor);
                }
            }
        }
        providers::add_scan_range(&mut coverage.public_scan_ranges, head.scan_range);
        records.extend(head.records);
        if let Some(cursor) = coverage.head_cursor.clone() {
            let page = providers.page(&candidate, Some(&cursor)).await?;
            let caught_up = page.exhausted
                || if matches!(candidate.chain, Chain::Bnb) {
                    providers::range_covered(&page, &snapshot.coverage.public_scan_ranges)
                } else {
                    providers::head_covered(&page, &known)
                };
            coverage.head_cursor = if caught_up {
                if coverage.head_cursors.is_empty() {
                    None
                } else {
                    Some(coverage.head_cursors.remove(0))
                }
            } else {
                page.cursor
            };
            coverage.head_complete = coverage.head_cursor.is_none();
            coverage.pages += 1;
            providers::add_scan_range(&mut coverage.public_scan_ranges, page.scan_range);
            records.extend(page.records);
        } else if !coverage.backfill_done {
            if let Some(cursor) = coverage.cursor.clone() {
                let page = providers.page(&candidate, Some(&cursor)).await?;
                coverage.cursor = page.cursor;
                coverage.backfill_done = page.exhausted;
                coverage.pages += 1;
                providers::add_scan_range(&mut coverage.public_scan_ranges, page.scan_range);
                records.extend(page.records);
            }
        }
        // Bound a page's references without silently advancing beyond omitted
        // evidence. The cursor is saved only if every reference fits storage.
        let new = records
            .iter()
            .filter(|r| !known.contains(&r.id))
            .map(|r| &r.id)
            .collect::<BTreeSet<_>>()
            .len();
        if snapshot.records.len() + new > self.record_limit {
            let mut previous = snapshot.coverage.clone();
            previous.history_complete = false;
            previous.notes.push(
                "A new page exceeds the record budget. Its continuation was not advanced.".into(),
            );
            store.save_page(&candidate, &[], &previous, &snapshot.balances)?;
            self.update_analysis(candidate.chain, &candidate.wallet, now())
                .await?;
            return Err("The next history page exceeds the wallet record budget; continuation was not advanced.".into());
        }
        if new > 0 {
            coverage.history_complete = false;
        }
        store.save_page(&candidate, &records, &coverage, &snapshot.balances)?;
        let refreshed = self.snapshot(candidate.chain, &candidate.wallet).await?;
        let mut pending: Vec<_> = refreshed
            .records
            .iter()
            .filter(|r| {
                r.transaction
                    .as_ref()
                    .is_none_or(|t| !t.finalized || !t.movement_complete)
            })
            .filter(|r| {
                let key = format!(
                    "retry:{}:{}:{}",
                    candidate.chain.key(),
                    candidate.wallet,
                    r.id
                );
                store
                    .state(&key)
                    .ok()
                    .flatten()
                    .and_then(|v| v.parse::<u64>().ok())
                    .is_none_or(|t| now() >= t)
            })
            .cloned()
            .collect();
        pending.sort_by_key(|r| std::cmp::Reverse(providers::record_priority(r)));
        pending.truncate(3);
        let mut fetched = Vec::new();
        for mut record in pending {
            match providers.fetch_record(&candidate, &record).await {
                Ok(value) => {
                    if value
                        .transaction
                        .as_ref()
                        .is_some_and(|t| !t.movement_complete)
                    {
                        let key = format!(
                            "retry:{}:{}:{}",
                            candidate.chain.key(),
                            candidate.wallet,
                            record.id
                        );
                        store.set_state(&key, &(now() + 3600).to_string())?;
                    }
                    fetched.push(value);
                }
                Err(error) => {
                    let key = format!(
                        "retry:{}:{}:{}",
                        candidate.chain.key(),
                        candidate.wallet,
                        record.id
                    );
                    store.set_state(&key, &(now() + 3600).to_string())?;
                    record.error = Some(error);
                    fetched.push(record);
                }
            }
        }
        store.save_page(&candidate, &fetched, &coverage, &snapshot.balances)?;
        let refreshed = self.snapshot(candidate.chain, &candidate.wallet).await?;
        let assets: BTreeSet<_> = refreshed
            .records
            .iter()
            .filter_map(|r| r.transaction.as_ref())
            .flat_map(|t| t.assets.iter().map(|d| d.asset.clone()))
            .collect();
        let balances = providers.balances(&candidate, &assets).await;
        let paid_fee = refreshed
            .records
            .iter()
            .filter_map(|r| r.transaction.as_ref())
            .any(|t| {
                t.fee_asset == "SOL"
                    && t.fee_quantity
                        .is_some_and(|q| q > rust_decimal::Decimal::ZERO)
            });
        match providers.execution_account(&candidate, paid_fee).await {
            Ok(verified) => {
                coverage.execution_account_verified = verified;
                if !verified {
                    coverage.notes.push("Execution account requires a supported ownership/fee adapter; program and protocol contracts are not trader ranks.".into());
                }
            }
            Err(error) => {
                coverage.execution_account_verified = false;
                coverage.notes.push(error);
            }
        }
        coverage.balances_reconciled = balances.is_ok();
        if let Err(error) = &balances {
            coverage.notes.push(error.clone());
        }
        let balances = balances.unwrap_or(snapshot.balances);
        let txs: Vec<_> = refreshed
            .records
            .iter()
            .filter_map(|r| r.transaction.as_ref())
            .collect();
        coverage.pending_records = refreshed.records.len() - txs.len()
            + txs
                .iter()
                .filter(|t| !t.finalized || !t.movement_complete)
                .count();
        let errors: BTreeSet<_> = refreshed
            .records
            .iter()
            .filter_map(|r| r.error.as_ref())
            .collect();
        coverage.notes.extend(errors.into_iter().take(3).cloned());
        let explanations: BTreeSet<_> = txs.iter().flat_map(|t| t.notes.iter()).collect();
        coverage
            .notes
            .extend(explanations.into_iter().take(3).cloned());
        coverage.ordering_complete = txs.iter().all(|t| t.index.is_some());
        coverage.oldest_record_at = txs.iter().map(|t| t.timestamp).min();
        coverage.newest_record_at = txs.iter().map(|t| t.timestamp).max();
        let ownership_history = if matches!(candidate.chain, Chain::Solana) {
            !providers.helius_keys.is_empty() && txs.iter().all(|t| t.block >= 111_491_819)
        } else {
            matches!(candidate.chain, Chain::Robinhood)
                && providers.config.blockscout_api_key.is_some()
        };
        // Every listed record, the canonical order, ending balances, fee
        // attribution and supported execution semantics are additional gates.
        coverage.history_complete = coverage.backfill_done
            && coverage.head_complete
            && coverage.pending_records == 0
            && ownership_history;
        let valuation_end = now();
        coverage.last_collected_at = Some(valuation_end);
        store.save_page(&candidate, &[], &coverage, &balances)?;
        self.price(&refreshed, &assets, valuation_end).await?;
        self.update_analysis(candidate.chain, &candidate.wallet, valuation_end)
            .await?;
        store.set_state("collector_error", "")?;
        Ok(())
    }

    async fn price(
        &self,
        snapshot: &Snapshot,
        assets: &BTreeSet<String>,
        end: u64,
    ) -> Result<(), String> {
        let store = self.store.as_ref().unwrap();
        let boundaries = [
            end.saturating_sub(60 * DAY),
            end.saturating_sub(30 * DAY),
            end,
        ];
        let mut ranked: Vec<_> = assets
            .iter()
            .map(|asset| {
                let mut times: BTreeSet<_> = snapshot
                    .records
                    .iter()
                    .filter_map(|r| r.transaction.as_ref())
                    .filter(|t| t.assets.iter().any(|d| &d.asset == asset) || &t.fee_asset == asset)
                    .map(|t| t.timestamp)
                    .collect();
                times.extend(boundaries);
                (asset.clone(), times)
            })
            .collect();
        let fee_asset = native(snapshot.candidate.chain).to_string();
        if !assets.contains(&fee_asset) {
            let mut times: BTreeSet<_> = snapshot
                .records
                .iter()
                .filter_map(|r| r.transaction.as_ref())
                .map(|t| t.timestamp)
                .collect();
            times.extend(boundaries);
            ranked.push((fee_asset, times));
        }
        ranked.sort_by_key(|(asset, times)| {
            (
                !quote(snapshot.candidate.chain, asset),
                std::cmp::Reverse(times.len()),
            )
        });
        let prices = accounting::PriceIndex::new(&snapshot.prices);
        ranked = ranked
            .into_iter()
            .filter_map(|(asset, times)| {
                let times: BTreeSet<_> = times
                    .into_iter()
                    .filter(|t| prices.get(&asset, *t).is_none())
                    .collect();
                (!times.is_empty()).then_some((asset, times))
            })
            .collect();
        let key = format!(
            "price_cursor:{}:{}",
            snapshot.candidate.chain.key(),
            snapshot.candidate.wallet
        );
        let offset = store
            .state(&key)?
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        if !ranked.is_empty() {
            let count = ranked.len();
            ranked.rotate_left(offset % count);
            store.set_state(&key, &((offset + 4) % count).to_string())?;
        }
        for (asset, times) in ranked.into_iter().take(4) {
            let times: Vec<_> = times.into_iter().collect();
            if let Ok(prices) = self
                .gecko
                .historical_usd_prices(snapshot.candidate.chain, &asset, &times)
                .await
            {
                let prices: Vec<_> = prices
                    .into_iter()
                    .map(|(timestamp, p)| Price {
                        asset: asset.clone(),
                        timestamp,
                        usd: p.usd_price,
                        granularity: format!("{:?}", p.granularity).to_ascii_lowercase(),
                        source: "GeckoTerminal historical USD candle; estimated conversion".into(),
                    })
                    .collect();
                store.save_prices(snapshot.candidate.chain, &prices)?;
            }
        }
        Ok(())
    }
}
