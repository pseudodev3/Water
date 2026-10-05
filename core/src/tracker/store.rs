use super::budget::Lane;
use super::model::*;
use crate::model::Chain;
use rusqlite::{params, Connection, OptionalExtension};
use std::{collections::BTreeMap, path::Path, sync::Mutex};

pub struct Store {
    db: Mutex<Connection>,
    pub path: String,
    pub current_limit: u64,
}

impl Store {
    pub fn helius_credits(&self, timestamp: u64) -> Result<u64, String> {
        let db = self.db.lock().map_err(|_| "Wallet storage is busy.")?;
        credits_in_window(&db, timestamp / DAY)
    }

    /// Reserve before sending. Every configured Helius credential shares this
    /// Water budget; a credential list does not multiply provider plan credits.
    #[cfg(test)]
    pub fn reserve_http(
        &self,
        timestamp: u64,
        daily_limit: u64,
        helius_limit: Option<u64>,
    ) -> Result<(), String> {
        self.reserve_http_in_lane(timestamp, daily_limit, helius_limit, None)
    }

    pub fn reserve_http_in_lane(
        &self,
        timestamp: u64,
        daily_limit: u64,
        helius_limit: Option<u64>,
        lane: Option<Lane>,
    ) -> Result<(), String> {
        self.reserve_work(
            timestamp,
            daily_limit,
            helius_limit.map(|limit| (limit, 10)),
            lane,
        )
    }

    pub fn reserve_work(
        &self,
        timestamp: u64,
        daily_limit: u64,
        helius: Option<(u64, u64)>,
        lane: Option<Lane>,
    ) -> Result<(), String> {
        let mut db = self.db.lock().map_err(|_| "Wallet storage is busy.")?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let day = timestamp / DAY;
        let read = |key: &str| -> Result<u64, String> {
            tx.query_row(
                "SELECT CAST(value AS INTEGER) FROM state WHERE key=?",
                [key],
                |r| r.get(0),
            )
            .optional()
            .map(|v| v.unwrap_or(0))
            .map_err(|e| e.to_string())
        };
        let used = read(&format!("requests:{day}"))?;
        let current_used = read(&format!("lane:current:{day}"))?;
        let current = matches!(lane, Some(Lane::Current));
        if (current && current_used >= self.current_limit)
            || (!current && used.saturating_sub(current_used) >= daily_limit)
        {
            return Err(
                "This work reached its daily request allocation; saved evidence is retained."
                    .into(),
            );
        }
        if let Some(lane) = lane {
            let limit = if current {
                self.current_limit
            } else {
                daily_limit
            };
            let count = read(&format!("lane:{}:{day}", lane.key()))?;
            if count >= lane.allowance(timestamp, limit) {
                return Err(format!(
                    "{} work is paced; next attempt is eligible at UTC timestamp {}.",
                    lane.key(),
                    lane.next_attempt(timestamp, limit, count)
                ));
            }
        }
        if let Some((limit, cost)) = helius {
            if credits_in_window(&tx, day)?.saturating_add(cost) > limit {
                return Err("Helius collection reached Water's rolling 31-day credit budget; saved evidence and cursors are retained.".into());
            }
            if let Some(lane) = lane {
                let key = format!("helius-lane:{}:{day}", lane.key());
                let daily = limit / 32; // Window counts up to 32 UTC date buckets.
                let share = match lane {
                    Lane::Current => 80,
                    Lane::History => 15,
                    Lane::Discovery => 5,
                };
                let cap = (daily * share / 100).max(cost);
                let allowance = if matches!(lane, Lane::History) {
                    let burst = cap / 4;
                    burst + (cap - burst) * (timestamp % DAY) / DAY
                } else {
                    cap
                };
                let count = read(&key)?;
                if count.saturating_add(cost) > allowance {
                    return Err(format!("Helius {} work is paced within its daily credit allocation; saved evidence is retained.",lane.key()));
                }
                tx.execute("INSERT INTO state(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,(count+cost).to_string()]).map_err(|e|e.to_string())?;
            }
            tx.execute("INSERT INTO state(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=CAST(value AS INTEGER)+CAST(excluded.value AS INTEGER)",params![format!("helius-credits:{day}"),cost.to_string()]).map_err(|e|e.to_string())?;
        }
        if let Some(lane) = lane {
            tx.execute("INSERT INTO state(key,value) VALUES(?,1) ON CONFLICT(key) DO UPDATE SET value=CAST(value AS INTEGER)+1",[format!("lane:{}:{day}",lane.key())]).map_err(|e|e.to_string())?;
        }
        tx.execute("INSERT INTO state(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![format!("requests:{day}"),(used+1).to_string()]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn open(path: &str) -> Result<Self, String> {
        if path != ":memory:" {
            if let Some(parent) = Path::new(path)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
            {
                std::fs::create_dir_all(parent)
                    .map_err(|_| "Could not create wallet evidence directory.")?;
            }
        }
        let db = Connection::open(path).map_err(|_| "Could not open wallet evidence storage.")?;
        db.busy_timeout(std::time::Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
          CREATE TABLE IF NOT EXISTS wallets(chain TEXT NOT NULL, wallet TEXT NOT NULL, candidate TEXT NOT NULL, coverage TEXT NOT NULL, balances TEXT NOT NULL DEFAULT '{}', analysis TEXT, scheduled_at INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(chain,wallet));
          CREATE TABLE IF NOT EXISTS records(chain TEXT NOT NULL, wallet TEXT NOT NULL, id TEXT NOT NULL, record TEXT NOT NULL, ready INTEGER NOT NULL, PRIMARY KEY(chain,wallet,id), FOREIGN KEY(chain,wallet) REFERENCES wallets(chain,wallet));
          CREATE TABLE IF NOT EXISTS prices(chain TEXT NOT NULL, asset TEXT NOT NULL, timestamp INTEGER NOT NULL, price TEXT NOT NULL, PRIMARY KEY(chain,asset,timestamp));
          CREATE TABLE IF NOT EXISTS token_quotes(chain TEXT NOT NULL,asset TEXT NOT NULL,quote TEXT NOT NULL, PRIMARY KEY(chain,asset));
          CREATE TABLE IF NOT EXISTS state(key TEXT PRIMARY KEY,value TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS pending_records ON records(chain,wallet,ready);").map_err(|e| e.to_string())?;
        Ok(Self {
            db: Mutex::new(db),
            path: path.into(),
            current_limit: std::env::var("WATER_TRACKER_CURRENT_DAILY_REQUESTS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(24000u64)
                .clamp(100, 96000),
        })
    }

    pub fn nominate(&self, candidate: Candidate, limit: usize) -> Result<bool, String> {
        let mut db = self.db.lock().map_err(|_| "Wallet storage is busy.")?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let key = candidate.chain.key();
        let existing: Option<String> = tx
            .query_row(
                "SELECT candidate FROM wallets WHERE chain=? AND wallet=?",
                params![key, candidate.wallet],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(value) = existing {
            let mut previous: Candidate =
                serde_json::from_str(&value).map_err(|e| e.to_string())?;
            for token in candidate.observed_tokens {
                if previous.observed_tokens.len() < 64 && !previous.observed_tokens.contains(&token)
                {
                    previous.observed_tokens.push(token);
                }
            }
            for source in candidate.sources {
                if let Some(old) = previous
                    .sources
                    .iter_mut()
                    .find(|s| s.name == source.name && s.profile == source.profile)
                {
                    *old = source;
                } else {
                    previous.sources.push(source);
                }
            }
            tx.execute(
                "UPDATE wallets SET candidate=? WHERE chain=? AND wallet=?",
                params![
                    serde_json::to_string(&previous).unwrap(),
                    key,
                    previous.wallet
                ],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(false);
        }
        let count: usize = tx
            .query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if count >= limit {
            return Err(
                "The free collection cohort is full. Existing wallets continue collecting.".into(),
            );
        }
        tx.execute(
            "INSERT INTO wallets(chain,wallet,candidate,coverage) VALUES(?,?,?,?)",
            params![
                key,
                candidate.wallet,
                serde_json::to_string(&candidate).unwrap(),
                serde_json::to_string(&Coverage::default()).unwrap()
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(true)
    }

    pub fn next_wallet(&self, timestamp: u64, interval: u64) -> Result<Option<Candidate>, String> {
        let mut db = self.db.lock().map_err(|_| "Wallet storage is busy.")?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let value: Option<String> = tx
            .query_row(
                "SELECT candidate FROM wallets WHERE scheduled_at<=? ORDER BY scheduled_at,chain,wallet LIMIT 1",
                [timestamp],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let candidate = value
            .map(|v| serde_json::from_str::<Candidate>(&v))
            .transpose()
            .map_err(|e| e.to_string())?;
        if let Some(c) = &candidate {
            tx.execute(
                "UPDATE wallets SET scheduled_at=? WHERE chain=? AND wallet=?",
                params![timestamp.saturating_add(interval), c.chain.key(), c.wallet],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(candidate)
    }

    pub fn wallet_count(&self) -> Result<usize, String> {
        self.db
            .lock()
            .map_err(|_| "Wallet storage is busy.")?
            .query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0))
            .map_err(|e| e.to_string())
    }

    pub fn lane_used(&self, lane: Lane, timestamp: u64) -> Result<u64, String> {
        Ok(self
            .state(&format!("lane:{}:{}", lane.key(), timestamp / DAY))?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0))
    }

    pub fn next_history_wallet(&self, timestamp: u64) -> Result<Option<Candidate>, String> {
        let mut db = self.db.lock().map_err(|_| "Wallet storage is busy.")?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        let value: Option<String> = tx.query_row("SELECT w.candidate FROM wallets w LEFT JOIN state s ON s.key='history-due:'||w.chain||':'||w.wallet WHERE CAST(COALESCE(s.value,'0') AS INTEGER)<=? ORDER BY CAST(COALESCE(s.value,'0') AS INTEGER),w.chain,w.wallet LIMIT 1",[timestamp],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let candidate: Option<Candidate> = value
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| e.to_string())?;
        if let Some(c) = &candidate {
            tx.execute("INSERT INTO state(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![format!("history-due:{}:{}",c.chain.key(),c.wallet),(timestamp+300).to_string()]).map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(candidate)
    }

    pub fn snapshot(&self, chain: Chain, wallet: &str) -> Result<Option<Snapshot>, String> {
        let db = self.db.lock().map_err(|_| "Wallet storage is busy.")?;
        let tuple: Option<(String, String, String)> = db
            .query_row(
                "SELECT candidate,coverage,balances FROM wallets WHERE chain=? AND wallet=?",
                params![chain.key(), wallet],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some((candidate, coverage, balances)) = tuple else {
            return Ok(None);
        };
        let mut statement = db
            .prepare("SELECT record FROM records WHERE chain=? AND wallet=? ORDER BY id")
            .map_err(|e| e.to_string())?;
        let records = statement
            .query_map(params![chain.key(), wallet], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .map(|r| {
                r.map_err(|e| e.to_string())
                    .and_then(|v| serde_json::from_str(&v).map_err(|e| e.to_string()))
            })
            .collect::<Result<Vec<Record>, String>>()?;
        let mut statement = db
            .prepare("SELECT price FROM prices WHERE chain=?")
            .map_err(|e| e.to_string())?;
        let prices = statement
            .query_map([chain.key()], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .map(|r| {
                r.map_err(|e| e.to_string())
                    .and_then(|v| serde_json::from_str(&v).map_err(|e| e.to_string()))
            })
            .collect::<Result<Vec<Price>, String>>()?;
        Ok(Some(Snapshot {
            candidate: serde_json::from_str(&candidate).map_err(|e| e.to_string())?,
            coverage: serde_json::from_str(&coverage).map_err(|e| e.to_string())?,
            records,
            prices,
            balances: serde_json::from_str(&balances).map_err(|e| e.to_string())?,
        }))
    }

    /// Evidence rows and the continuation cursor commit together. A restart
    /// cannot advance past a page whose transaction references were not saved.
    pub fn save_page(
        &self,
        candidate: &Candidate,
        records: &[Record],
        coverage: &Coverage,
        balances: &BTreeMap<String, rust_decimal::Decimal>,
    ) -> Result<(), String> {
        let mut db = self.db.lock().map_err(|_| "Wallet storage is busy.")?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        for record in records {
            tx.execute("INSERT INTO records(chain,wallet,id,record,ready) VALUES(?,?,?,?,?) ON CONFLICT(chain,wallet,id) DO UPDATE SET record=excluded.record,ready=excluded.ready WHERE excluded.ready=1 OR records.ready=0", params![candidate.chain.key(), candidate.wallet, record.id, serde_json::to_string(record).unwrap(), record.transaction.is_some() as i32]).map_err(|e| e.to_string())?;
        }
        tx.execute(
            "UPDATE wallets SET coverage=?,balances=? WHERE chain=? AND wallet=?",
            params![
                serde_json::to_string(coverage).unwrap(),
                serde_json::to_string(balances).unwrap(),
                candidate.chain.key(),
                candidate.wallet
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn save_prices(&self, chain: Chain, prices: &[Price]) -> Result<(), String> {
        let mut db = self.db.lock().map_err(|_| "Wallet storage is busy.")?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        for price in prices {
            tx.execute(
                "INSERT OR REPLACE INTO prices(chain,asset,timestamp,price) VALUES(?,?,?,?)",
                params![
                    chain.key(),
                    price.asset,
                    price.timestamp,
                    serde_json::to_string(price).unwrap()
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn token_quotes(
        &self,
        chain: Chain,
    ) -> Result<BTreeMap<String, crate::model::TokenQuote>, String> {
        let db = self.db.lock().map_err(|_| "Wallet storage is busy.")?;
        let mut stmt = db
            .prepare("SELECT quote FROM token_quotes WHERE chain=?")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([chain.key()], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.map(|r| {
            let quote: crate::model::TokenQuote =
                serde_json::from_str(&r.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            Ok((quote.asset.clone(), quote))
        })
        .collect()
    }
    pub fn save_token_quotes(
        &self,
        chain: Chain,
        quotes: &[crate::model::TokenQuote],
    ) -> Result<(), String> {
        let mut db = self.db.lock().map_err(|_| "Wallet storage is busy.")?;
        let tx = db.transaction().map_err(|e| e.to_string())?;
        for quote in quotes {
            tx.execute("INSERT INTO token_quotes(chain,asset,quote) VALUES(?,?,?) ON CONFLICT(chain,asset) DO UPDATE SET quote=excluded.quote",params![chain.key(),quote.asset,serde_json::to_string(quote).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    pub fn save_analysis(&self, analysis: &Analysis) -> Result<(), String> {
        self.db
            .lock()
            .map_err(|_| "Wallet storage is busy.")?
            .execute(
                "UPDATE wallets SET analysis=? WHERE chain=? AND wallet=?",
                params![
                    serde_json::to_string(analysis).unwrap(),
                    analysis.candidate.chain.key(),
                    analysis.candidate.wallet
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn analyses(&self) -> Result<Vec<Analysis>, String> {
        let db = self.db.lock().map_err(|_| "Wallet storage is busy.")?;
        let mut statement=db.prepare("SELECT analysis,coverage FROM wallets WHERE analysis IS NOT NULL ORDER BY chain,wallet").map_err(|e|e.to_string())?;
        let result = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| e.to_string())?
            .map(|r| {
                let (value, coverage) = r.map_err(|e| e.to_string())?;
                let mut analysis: Analysis =
                    serde_json::from_str(&value).map_err(|e| e.to_string())?;
                let current: Coverage =
                    serde_json::from_str(&coverage).map_err(|e| e.to_string())?;
                if analysis.status.starts_with("qualified_")
                    && (!current.history_complete
                        || !current.head_complete
                        || !current.execution_account_verified
                        || current.last_collected_at != analysis.coverage.last_collected_at)
                {
                    analysis.status = "incomplete".into();
                    analysis.coverage = current;
                    for window in &mut analysis.windows {
                        window.qualified = false;
                        for gate in &mut window.gates {
                            if gate.name == "Complete history" {
                                gate.passed = false;
                            }
                        }
                    }
                }
                Ok(analysis)
            })
            .collect();
        result
    }

    pub fn state(&self, key: &str) -> Result<Option<String>, String> {
        self.db
            .lock()
            .map_err(|_| "Wallet storage is busy.")?
            .query_row("SELECT value FROM state WHERE key=?", [key], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())
    }

    pub fn set_state(&self, key: &str, value: &str) -> Result<(), String> {
        self.db
            .lock()
            .map_err(|_| "Wallet storage is busy.")?
            .execute(
                "INSERT OR REPLACE INTO state(key,value) VALUES(?,?)",
                params![key, value],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM wallets", [], |r| r.get(0))
            .unwrap()
    }
}

fn credits_in_window(db: &Connection, day: u64) -> Result<u64, String> {
    db.query_row("SELECT COALESCE(SUM(CAST(value AS INTEGER)),0) FROM state WHERE key LIKE 'helius-credits:%' AND CAST(substr(key,length('helius-credits:')+1) AS INTEGER) BETWEEN ? AND ?", params![day.saturating_sub(31),day], |r|r.get(0)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paced_history_cannot_spend_the_current_checks_allocation_over_a_full_utc_day() {
        let store = Store::open(":memory:").unwrap();
        let start = 100 * DAY;
        let mut current = 0;
        for minute in 0..1440 {
            let timestamp = start + minute * 60;
            if minute % 10 == 0 {
                // Captured production cohort's bounded head routes: seven SOL
                // wallets (2 calls each), two RH wallets (4 each), two BNB
                // wallets with known tokens (4 each), and one tokenless BNB
                // nomination (chain check only; cannot be marked fresh).
                for _ in 0..31 {
                    store
                        .reserve_http_in_lane(timestamp, 2000, None, Some(Lane::Current))
                        .unwrap();
                    current += 1;
                }
            }
            while store
                .reserve_http_in_lane(timestamp, 2000, Some(800_000), Some(Lane::History))
                .is_ok()
            {}
        }
        assert_eq!(current, 4464);
        assert_eq!(store.lane_used(Lane::Current, start).unwrap(), 4464);
        assert!(store.lane_used(Lane::History, start).unwrap() <= 375);
        assert!(
            store
                .state("requests:100")
                .unwrap()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                <= 6500
        );
    }

    #[test]
    fn new_work_allocations_never_reset_legacy_spending_or_charge_a_failed_credit_reservation() {
        let store = Store::open(":memory:").unwrap();
        for _ in 0..2000 {
            store.reserve_http(100 * DAY, 2000, None).unwrap();
        }
        store
            .reserve_work(100 * DAY, 2000, Some((800000, 11)), Some(Lane::Current))
            .unwrap();
        assert_eq!(store.lane_used(Lane::Current, 100 * DAY).unwrap(), 1);
        assert_eq!(
            store.state("requests:100").unwrap().as_deref(),
            Some("2001")
        );
        assert_eq!(store.helius_credits(100 * DAY).unwrap(), 11);
        assert!(store
            .reserve_work(100 * DAY, 2000, None, Some(Lane::History))
            .is_err());
        // Rejected provider-credit reservations change neither HTTP nor lane totals.
        assert!(store
            .reserve_work(100 * DAY, 2000, Some((11, 1)), Some(Lane::Current))
            .is_err());
        assert_eq!(store.lane_used(Lane::Current, 100 * DAY).unwrap(), 1);
        assert_eq!(
            store.state("requests:100").unwrap().as_deref(),
            Some("2001")
        );
    }

    #[test]
    fn current_check_scheduling_is_fair_and_does_not_poll_again_before_the_due_time() {
        let store = Store::open(":memory:").unwrap();
        let a = candidate();
        let mut b = a.clone();
        b.wallet = "second-test-wallet".into();
        store.nominate(a.clone(), 2).unwrap();
        store.nominate(b.clone(), 2).unwrap();
        assert_eq!(
            store.next_wallet(100, 2400).unwrap().unwrap().wallet,
            a.wallet
        );
        assert_eq!(
            store.next_wallet(160, 2400).unwrap().unwrap().wallet,
            b.wallet
        );
        assert!(store.next_wallet(2400, 2400).unwrap().is_none());
        assert_eq!(
            store.next_wallet(2500, 2400).unwrap().unwrap().wallet,
            a.wallet
        );
        // Historical work has its own fair schedule; it cannot move head due times.
        assert!(store.next_history_wallet(2501).unwrap().is_some());
        assert_eq!(
            store.next_wallet(2560, 2400).unwrap().unwrap().wallet,
            b.wallet
        );
    }
    #[test]
    fn credit_limits_are_atomic_persisted_and_span_billing_month_boundaries() {
        let path =
            std::env::temp_dir().join(format!("water-credits-{}.sqlite", std::process::id()));
        let path = path.to_str().unwrap();
        let db = Store::open(path).unwrap();
        db.reserve_http(100 * DAY, 3, Some(20)).unwrap();
        db.reserve_http(101 * DAY, 3, Some(20)).unwrap();
        assert_eq!(db.helius_credits(130 * DAY).unwrap(), 20);
        assert!(db.reserve_http(130 * DAY, 3, Some(20)).is_err());
        assert_eq!(db.state("requests:130").unwrap(), None);
        drop(db);
        let db = Store::open(path).unwrap();
        assert_eq!(db.helius_credits(131 * DAY).unwrap(), 20);
        assert_eq!(db.helius_credits(132 * DAY).unwrap(), 10);
        db.reserve_http(132 * DAY, 3, Some(20)).unwrap();
        assert!(db.reserve_http(132 * DAY, 3, Some(20)).is_err());
        // Public/non-Helius reads consume the daily request limit only.
        db.reserve_http(132 * DAY, 3, None).unwrap();
        db.reserve_http(132 * DAY, 3, None).unwrap();
        assert!(db.reserve_http(132 * DAY, 3, None).is_err());
        assert_eq!(db.helius_credits(132 * DAY).unwrap(), 20);
        drop(db);
        let _ = std::fs::remove_file(path);
    }
    fn candidate() -> Candidate {
        Candidate {
            observed_tokens: Vec::new(),
            chain: Chain::Solana,
            wallet: "21rgbFW6sujQovCw3qt6R2EdE97Yzzvk8sSc37Bb72Cm".into(),
            discovered_at: 1,
            sources: vec![Source {
                name: "test".into(),
                observed_at: 1,
                detail: "synthetic test".into(),
                profile: None,
            }],
        }
    }
    #[test]
    fn restarts_preserve_evidence_cursor_and_deduplicate_identity() {
        let path = std::env::temp_dir().join(format!(
            "water-tracker-{}-{}.sqlite",
            std::process::id(),
            now()
        ));
        let c = candidate();
        {
            let store = Store::open(path.to_str().unwrap()).unwrap();
            store.nominate(c.clone(), 10).unwrap();
            store.nominate(c.clone(), 10).unwrap();
            assert_eq!(store.count(), 1);
            let coverage = Coverage {
                cursor: Some("page-2".into()),
                ..Default::default()
            };
            store
                .save_page(
                    &c,
                    &[Record {
                        id: "tx".into(),
                        raw: serde_json::json!({"fixture":true}),
                        transaction: None,
                        error: None,
                    }],
                    &coverage,
                    &BTreeMap::new(),
                )
                .unwrap();
        }
        {
            let store = Store::open(path.to_str().unwrap()).unwrap();
            let snapshot = store.snapshot(c.chain, &c.wallet).unwrap().unwrap();
            assert_eq!(snapshot.coverage.cursor.as_deref(), Some("page-2"));
            assert_eq!(snapshot.records.len(), 1);
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn a_committed_history_gap_immediately_demotes_a_saved_rank() {
        let store = Store::open(":memory:").unwrap();
        let c = candidate();
        store.nominate(c.clone(), 10).unwrap();
        let complete = Coverage {
            history_complete: true,
            head_complete: true,
            ..Default::default()
        };
        store
            .save_page(&c, &[], &complete, &BTreeMap::new())
            .unwrap();
        let mut analysis = super::super::accounting::analyze(
            store.snapshot(c.chain, &c.wallet).unwrap().unwrap(),
            now(),
        );
        analysis.status = "qualified_60d".into();
        for w in &mut analysis.windows {
            w.qualified = true;
        }
        store.save_analysis(&analysis).unwrap();
        store
            .save_page(
                &c,
                &[],
                &Coverage {
                    history_complete: false,
                    head_complete: false,
                    ..complete
                },
                &BTreeMap::new(),
            )
            .unwrap();
        let result = store.analyses().unwrap();
        assert_eq!(result[0].status, "incomplete");
        assert!(result[0].windows.iter().all(|w| !w.qualified));
    }
}
