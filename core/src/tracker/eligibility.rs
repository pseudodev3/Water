//! Capital screening stays separate from historical economics and profitability.
use super::model::Candidate;
use crate::model::TokenQuote;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Inventory {
    pub quantities: BTreeMap<String, Decimal>,
    pub observed_at: u64,
    pub block: String,
    pub source: String,
    pub complete: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WalletValue {
    pub status: String,
    pub minimum_usd: Decimal,
    #[serde(default)]
    pub maximum_usd: Option<Decimal>,
    pub known_value_usd: Option<Decimal>,
    pub total_complete: bool,
    pub inventory_complete: bool,
    pub positive_assets: usize,
    pub unpriced_assets: usize,
    pub balance_observed_at: Option<u64>,
    pub oldest_price_at: Option<u64>,
    pub balance_block: Option<String>,
    pub source: Option<String>,
    pub next_check_at: u64,
    pub detail: String,
}

pub fn key(candidate: &Candidate) -> String {
    format!("{}:{}", candidate.chain.key(), candidate.wallet)
}

pub fn assess(
    inventory: Option<&Inventory>,
    quotes: &BTreeMap<String, TokenQuote>,
    minimum: Decimal,
    maximum: Decimal,
    at: u64,
    next_check_at: u64,
) -> WalletValue {
    let (mut value, mut positive, mut missing, mut priced) = (Decimal::ZERO, 0, 0, 0);
    let mut oldest = None;
    let fresh =
        inventory.is_some_and(|i| i.error.is_none() && at.saturating_sub(i.observed_at) <= 3600);
    if let Some(i) = inventory {
        for (asset, quantity) in &i.quantities {
            if *quantity <= Decimal::ZERO {
                continue;
            }
            positive += 1;
            let mark = quotes
                .get(asset)
                .filter(|q| fresh && at.saturating_sub(q.observed_at) <= 900);
            let usd = mark
                .and_then(|q| q.price_usd)
                .filter(|p| *p >= Decimal::ZERO)
                .and_then(|p| p.checked_mul(*quantity));
            if let Some(usd) = usd.and_then(|usd| value.checked_add(usd)) {
                value = usd;
                priced += 1;
                oldest = Some(oldest.map_or(mark.unwrap().observed_at, |old: u64| {
                    old.min(mark.unwrap().observed_at)
                }));
            } else {
                missing += 1;
            }
        }
    }
    let complete = fresh && inventory.is_some_and(|i| i.complete) && missing == 0;
    let known = (fresh && (priced > 0 || complete)).then_some(value);
    let maximum = (maximum > Decimal::ZERO).then_some(maximum);
    let status = if known.is_some_and(|v| maximum.is_some_and(|max| v > max)) {
        "above_maximum"
    } else if (minimum <= Decimal::ZERO && maximum.is_none()) || known.is_some_and(|v| v >= minimum)
    {
        "eligible"
    } else if complete {
        "below_minimum"
    } else {
        "awaiting_value"
    };
    WalletValue {
        status: status.into(), minimum_usd: minimum, maximum_usd: maximum, known_value_usd: known,
        total_complete: complete, inventory_complete: inventory.is_some_and(|i| i.complete),
        positive_assets: positive, unpriced_assets: missing,
        balance_observed_at: inventory.map(|i| i.observed_at), oldest_price_at: oldest,
        balance_block: inventory.map(|i| i.block.clone()), source: inventory.map(|i| i.source.clone()), next_check_at,
        detail: inventory.and_then(|i| i.error.clone()).unwrap_or_else(|| match status {
            "above_maximum" => "Received priced holdings alone exceed the collection ceiling. History collection is paused; saved evidence is retained.".into(),
            "eligible" if complete => "Received native and token holdings meet the collection range; this does not verify profitability.".into(),
            "eligible" if maximum.is_some() => "Received priced holdings meet the minimum and do not prove a total above the ceiling. This is a lower bound; the full total and upper limit remain unverified.".into(),
            "eligible" => "Received priced holdings alone meet the collection minimum. This is a lower bound; other assets may be missing or unpriced.".into(),
            "below_minimum" => "The received complete wallet-token inventory is below the collection minimum. History collection is paused; saved evidence is retained.".into(),
            _ => "Wallet value is not established. Expensive history work is paused while bounded balance and market checks continue. Missing prices are not zero.".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mark(asset: &str, price: Option<Decimal>, at: u64) -> TokenQuote {
        TokenQuote {
            asset: asset.into(),
            price_usd: price,
            observed_at: at,
            ..Default::default()
        }
    }
    #[test]
    fn native_and_tokens_use_exact_boundary_and_unknowns_never_prove_below() {
        let at = 10000;
        let mut i = Inventory {
            quantities: BTreeMap::from([
                ("SOL".into(), Decimal::from(5)),
                ("token".into(), Decimal::from(100)),
            ]),
            observed_at: at,
            complete: true,
            ..Default::default()
        };
        let mut quotes = BTreeMap::from([
            ("SOL".into(), mark("SOL", Some(Decimal::from(100)), at)),
            ("token".into(), mark("token", Some(Decimal::from(5)), at)),
        ]);
        let minimum = Decimal::from(1000);
        assert_eq!(
            assess(Some(&i), &quotes, minimum, Decimal::ZERO, at, 0).status,
            "eligible"
        );
        i.quantities.insert("token".into(), Decimal::from(99));
        assert_eq!(
            assess(Some(&i), &quotes, minimum, Decimal::ZERO, at, 0).status,
            "below_minimum"
        );
        quotes.get_mut("token").unwrap().price_usd = None;
        let v = assess(Some(&i), &quotes, minimum, Decimal::ZERO, at, 0);
        assert_eq!(v.status, "awaiting_value");
        assert_eq!(v.known_value_usd, Some(Decimal::from(500)));
        i.quantities.insert("SOL".into(), Decimal::from(10));
        let v = assess(Some(&i), &quotes, minimum, Decimal::ZERO, at, 0);
        assert_eq!(v.status, "eligible");
        assert!(!v.total_complete);
        quotes.get_mut("SOL").unwrap().observed_at = at - 901;
        assert_eq!(
            assess(Some(&i), &quotes, minimum, Decimal::ZERO, at, 0).status,
            "awaiting_value"
        );
        assert_eq!(
            assess(Some(&i), &quotes, minimum, Decimal::ZERO, at + 3601, 0).known_value_usd,
            None
        );
        i.complete = false;
        i.quantities.clear();
        assert_eq!(
            assess(Some(&i), &quotes, minimum, Decimal::ZERO, at, 0).status,
            "awaiting_value"
        );
        i.complete = true;
        let v = assess(Some(&i), &quotes, minimum, Decimal::ZERO, at, 0);
        assert_eq!(v.status, "below_minimum");
        assert_eq!(v.known_value_usd, Some(Decimal::ZERO));
    }

    #[test]
    fn ceiling_uses_native_plus_tokens_inclusive_boundary_and_partial_lower_bounds() {
        let at = 10000;
        let minimum = Decimal::from(1000);
        let maximum = Decimal::from(50000);
        let mut i = Inventory {
            quantities: BTreeMap::from([
                ("SOL".into(), Decimal::from(10)),
                ("token".into(), Decimal::from(490)),
            ]),
            observed_at: at,
            complete: true,
            ..Default::default()
        };
        let mut quotes = BTreeMap::from([
            ("SOL".into(), mark("SOL", Some(Decimal::from(100)), at)),
            ("token".into(), mark("token", Some(Decimal::from(100)), at)),
        ]);
        let v = assess(Some(&i), &quotes, minimum, maximum, at, 0);
        assert_eq!(v.status, "eligible");
        assert_eq!(v.known_value_usd, Some(maximum));
        assert!(v.total_complete);
        assert_eq!(v.maximum_usd, Some(maximum));
        i.quantities
            .insert("token".into(), "490.0001".parse().unwrap());
        assert_eq!(
            assess(Some(&i), &quotes, minimum, maximum, at, 0).status,
            "above_maximum"
        );
        i.complete = false;
        assert_eq!(
            assess(Some(&i), &quotes, minimum, maximum, at, 0).status,
            "above_maximum"
        );
        quotes.get_mut("token").unwrap().price_usd = None;
        let v = assess(Some(&i), &quotes, minimum, maximum, at, 0);
        assert_eq!(v.status, "eligible");
        assert!(!v.total_complete);
        assert!(v.detail.contains("upper limit remain unverified"));
        i.quantities.insert("SOL".into(), Decimal::from(501));
        assert_eq!(
            assess(Some(&i), &quotes, minimum, maximum, at, 0).status,
            "above_maximum"
        );
        assert_eq!(
            assess(Some(&i), &quotes, minimum, Decimal::ZERO, at, 0).status,
            "eligible"
        );
        quotes.get_mut("SOL").unwrap().observed_at = at - 901;
        assert_eq!(
            assess(Some(&i), &quotes, minimum, maximum, at, 0).status,
            "awaiting_value"
        );
        assert_eq!(
            assess(None, &quotes, Decimal::ZERO, maximum, at, 0).status,
            "awaiting_value"
        );
    }
}
