use super::model::DAY;

#[derive(Clone, Copy, Debug)]
pub enum Lane {
    Current,
    History,
    Discovery,
}

impl Lane {
    pub fn key(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::History => "history",
            Self::Discovery => "discovery",
        }
    }
    pub fn limit(self, daily: u64) -> u64 {
        let current = 0;
        let discovery = daily * 5 / 100;
        match self {
            Self::Current => daily,
            Self::Discovery => discovery,
            Self::History => daily - current - discovery,
        }
    }
    pub fn allowance(self, timestamp: u64, daily: u64) -> u64 {
        let limit = self.limit(daily);
        if matches!(self, Self::History) {
            // History cannot spend its whole day's allocation at startup.
            // Allow a bounded initial batch, then release the remainder over UTC day.
            let burst = (limit / 4).max(1).min(limit);
            burst + (limit - burst) * (timestamp % DAY) / DAY
        } else {
            limit
        }
    }
    pub fn minimum_batch(self, daily: u64) -> u64 {
        if matches!(self, Self::History) {
            let limit = self.limit(daily);
            limit.min(25).min((limit / 4).max(1))
        } else {
            1
        }
    }
    pub fn next_attempt(self, timestamp: u64, daily: u64, used: u64) -> u64 {
        let start = timestamp / DAY * DAY;
        let limit = self.limit(daily);
        let batch = self.minimum_batch(daily);
        if used.saturating_add(batch) > limit || limit == 0 {
            return start + DAY;
        }
        let burst = (limit / 4).max(1).min(limit);
        if !matches!(self, Self::History)
            || self.allowance(timestamp, daily).saturating_sub(used) >= batch
        {
            return timestamp;
        }
        let needed = used + batch - burst;
        start + (needed * DAY).div_ceil(limit - burst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn historical_passes_wait_for_a_batch_without_disabling_small_budgets() {
        let start = 100 * DAY;
        assert_eq!(Lane::History.minimum_batch(2000), 25);
        assert_eq!(Lane::History.next_attempt(start, 2000, 475), start + 1516);
        assert_eq!(Lane::History.allowance(start + 1516, 2000), 500);
        assert_eq!(Lane::History.next_attempt(start, 2000, 1880), start + DAY);
        assert_eq!(Lane::History.minimum_batch(10), 2);
        assert_eq!(Lane::History.next_attempt(start, 10, 0), start);
        assert_eq!(Lane::Discovery.next_attempt(start, 10, 0), start + DAY);
    }
}
