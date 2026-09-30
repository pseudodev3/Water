use crate::model::{
    ChainEvidence, HolderEvidence, MarketSnapshot, PressureComponent, PressureDiagnostic,
    ScanRequest, ScanResponse, SourceStatus, TokenSnapshot,
};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn build_scan(
    request: &ScanRequest,
    market: Option<&MarketSnapshot>,
    holder_evidence: HolderEvidence,
    chain_evidence: ChainEvidence,
    mut sources: Vec<SourceStatus>,
) -> ScanResponse {
    let (effective_market_cap, market_cap_derived) =
        effective_market_cap(market, holder_evidence.total_supply);

    if market_cap_derived {
        sources.push(SourceStatus {
            source: "Market cap reconstruction".to_string(),
            ok: true,
            detail: "GeckoTerminal did not provide market cap, so Water derived it from current onchain token supply × observed USD price.".to_string(),
        });
    }

    let token = TokenSnapshot {
        name: market.and_then(|value| value.name.clone()),
        symbol: market.and_then(|value| value.symbol.clone()),
        price_usd: market.and_then(|value| value.price_usd),
        liquidity_usd: market.and_then(|value| value.liquidity_usd),
        market_cap_usd: effective_market_cap,
    };

    let concentration = holder_evidence.top_ten_percentage;
    let sell_share = market.and_then(recent_sell_share);
    let liquidity_coverage =
        market.and_then(|value| liquidity_coverage(value, effective_market_cap));

    let mut components = Vec::new();

    if let Some(value) = concentration {
        components.push(PressureComponent {
            key: "top_holder_concentration",
            label: "Top-holder concentration",
            observed: format!("{value:.1}%"),
            pressure: (value / 50.0).clamp(0.0, 1.0),
            detail: "Top-ten ownership divided by current token supply, observed from chain-native holder data.".to_string(),
        });
    }

    if let Some(value) = sell_share {
        components.push(PressureComponent {
            key: "recent_sell_share",
            label: "Recent sell share",
            observed: format!("{:.0}%", value * 100.0),
            pressure: value.clamp(0.0, 1.0),
            detail: "Sell transactions divided by buys + sells in the top GeckoTerminal pool over the last hour.".to_string(),
        });
    }

    if let Some(value) = liquidity_coverage {
        components.push(PressureComponent {
            key: "liquidity_coverage",
            label: "Liquidity coverage",
            observed: format!("{:.1}%", value * 100.0),
            pressure: (1.0 - (value / 0.20)).clamp(0.0, 1.0),
            detail: "Observed DEX liquidity divided by market cap when verified, otherwise FDV. Thin coverage raises exit fragility.".to_string(),
        });
    }

    let index = pressure_index(&components);
    let opponent_notes = opponent_notes(market, concentration, sell_share, liquidity_coverage);

    ScanResponse {
        chain: request.chain,
        address: request.address.trim().to_string(),
        scanned_at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default(),
        token,
        pressure: PressureDiagnostic {
            index,
            methodology: "Transparent diagnostic from chain-native holder concentration and public DEX market structure. It is not a probability, prediction, or buy/sell signal.",
            components,
        },
        opponent_notes,
        chain_evidence,
        holder_evidence,
        sources,
    }
}

fn recent_sell_share(market: &MarketSnapshot) -> Option<f64> {
    let buys = market.buys_h1?;
    let sells = market.sells_h1?;
    let total = buys + sells;

    (total > 0).then_some(sells as f64 / total as f64)
}

fn effective_market_cap(
    market: Option<&MarketSnapshot>,
    total_supply: Option<f64>,
) -> (Option<f64>, bool) {
    let Some(market) = market else {
        return (None, false);
    };

    if let Some(value) = market.market_cap_usd {
        return (Some(value), false);
    }

    let derived = match (market.price_usd, total_supply) {
        (Some(price), Some(supply))
            if price.is_finite() && supply.is_finite() && price >= 0.0 && supply > 0.0 =>
        {
            Some(price * supply)
        }
        _ => None,
    };

    (derived, derived.is_some())
}

fn liquidity_coverage(
    market: &MarketSnapshot,
    effective_market_cap: Option<f64>,
) -> Option<f64> {
    let liquidity = market.liquidity_usd?;
    let reference = effective_market_cap.or(market.fdv_usd)?;

    (reference > 0.0).then_some((liquidity / reference).max(0.0))
}

fn pressure_index(components: &[PressureComponent]) -> Option<u8> {
    if components.len() < 2 {
        return None;
    }

    let mut weighted = 0.0;
    let mut total_weight = 0.0;

    for component in components {
        let weight = match component.key {
            "top_holder_concentration" => 0.45,
            "recent_sell_share" => 0.30,
            "liquidity_coverage" => 0.25,
            _ => 0.0,
        };

        weighted += component.pressure * weight;
        total_weight += weight;
    }

    (total_weight > 0.0).then(|| {
        ((weighted / total_weight) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8
    })
}

fn opponent_notes(
    market: Option<&MarketSnapshot>,
    concentration: Option<f64>,
    sell_share: Option<f64>,
    coverage: Option<f64>,
) -> Vec<String> {
    let mut notes = Vec::new();

    if let Some(value) = concentration {
        notes.push(format!(
            "A large holder sees that the top ten addresses control about {value:.1}% of current supply."
        ));
    }

    if let (Some(market), Some(value)) = (market, sell_share) {
        notes.push(format!(
            "A short-term trader sees {:.0}% of the top pool's last-hour transactions on the sell side ({} buys, {} sells).",
            value * 100.0,
            market.buys_h1.unwrap_or_default(),
            market.sells_h1.unwrap_or_default()
        ));
    }

    if let Some(value) = coverage {
        notes.push(format!(
            "A holder thinking about exiting sees DEX liquidity equal to roughly {:.1}% of the token's market-cap/FDV reference.",
            value * 100.0
        ));
    }

    if let Some(market) = market {
        if let (Some(volume), Some(liquidity)) = (market.volume_h24_usd, market.liquidity_usd) {
            if liquidity > 0.0 {
                notes.push(format!(
                    "Twenty-four-hour volume is about {:.1}× current observed liquidity, a useful turnover context rather than a directional signal.",
                    volume / liquidity
                ));
            }
        }
    }

    if notes.is_empty() {
        notes.push(
            "Water does not have enough public evidence to construct an opponent view without guessing."
                .to_string(),
        );
    }

    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> MarketSnapshot {
        MarketSnapshot {
            name: Some("Fixture".to_string()),
            symbol: Some("FIX".to_string()),
            price_usd: Some(0.1),
            liquidity_usd: Some(10_000.0),
            market_cap_usd: Some(100_000.0),
            fdv_usd: Some(100_000.0),
            volume_h24_usd: Some(50_000.0),
            buys_h1: Some(40),
            sells_h1: Some(60),
        }
    }

    #[test]
    fn derives_market_structure_components() {
        let market = fixture();

        assert_eq!(recent_sell_share(&market), Some(0.6));
        assert_eq!(liquidity_coverage(&market, market.market_cap_usd), Some(0.1));
    }

    #[test]
    fn derives_market_cap_from_chain_supply_when_provider_omits_it() {
        let mut market = fixture();
        market.market_cap_usd = None;
        market.price_usd = Some(0.002);

        let (market_cap, derived) = effective_market_cap(Some(&market), Some(500_000_000.0));

        assert_eq!(market_cap, Some(1_000_000.0));
        assert!(derived);
    }

    #[test]
    fn provider_market_cap_wins_over_derived_supply_value() {
        let market = fixture();

        let (market_cap, derived) = effective_market_cap(Some(&market), Some(999_999_999.0));

        assert_eq!(market_cap, Some(100_000.0));
        assert!(!derived);
    }

    #[test]
    fn requires_multiple_components_before_scoring() {
        let component = PressureComponent {
            key: "top_holder_concentration",
            label: "Top-holder concentration",
            observed: "20%".to_string(),
            pressure: 0.4,
            detail: "fixture".to_string(),
        };

        assert_eq!(pressure_index(&[component]), None);
    }
}
