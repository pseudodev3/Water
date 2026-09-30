use crate::model::{
    ChainEvidence, PressureComponent, PressureDiagnostic, ScanRequest, ScanResponse, SourceStatus,
    TokenSnapshot,
};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn build_scan(
    request: &ScanRequest,
    info: Option<&Value>,
    pool: Option<&Value>,
    holders: Option<&Value>,
    traders: Option<&Value>,
    chain_evidence: ChainEvidence,
    sources: Vec<SourceStatus>,
) -> ScanResponse {
    let token = TokenSnapshot {
        name: info.and_then(|value| find_string(value, &["name", "token_name"])),
        symbol: info.and_then(|value| find_string(value, &["symbol", "token_symbol"])),
        price_usd: info.and_then(|value| find_number(value, &["price", "price_usd", "usd_price"])),
        liquidity_usd: pool
            .and_then(|value| find_number(value, &["liquidity", "liquidity_usd", "pool_liquidity"]))
            .or_else(|| info.and_then(|value| find_number(value, &["liquidity", "liquidity_usd"]))),
        market_cap_usd: info.and_then(|value| {
            find_number(
                value,
                &["market_cap", "marketcap", "market_cap_usd", "usd_market_cap"],
            )
        }),
    };

    let concentration = holders.and_then(top_ten_concentration);
    let profitable_share = holders.and_then(positive_pnl_share);
    let sell_dominant_share = traders.and_then(sell_dominant_share);

    let mut components = Vec::new();

    if let Some(value) = concentration {
        components.push(PressureComponent {
            key: "top_holder_concentration",
            label: "Top-holder concentration",
            observed: format!("{value:.1}%"),
            pressure: (value / 50.0).clamp(0.0, 1.0),
            detail: "Share held by the first ten observable holder rows returned by GMGN.".to_string(),
        });
    }

    if let Some(value) = profitable_share {
        components.push(PressureComponent {
            key: "profitable_holder_share",
            label: "Profitable holder share",
            observed: format!("{:.0}%", value * 100.0),
            pressure: value.clamp(0.0, 1.0),
            detail: "Share of observable top-holder rows with positive recorded realized or unrealized PnL.".to_string(),
        });
    }

    if let Some(value) = sell_dominant_share {
        components.push(PressureComponent {
            key: "sell_dominant_traders",
            label: "Sell-dominant traders",
            observed: format!("{:.0}%", value * 100.0),
            pressure: value.clamp(0.0, 1.0),
            detail: "Share of observable top-trader rows where current sell volume is above current buy volume.".to_string(),
        });
    }

    let index = pressure_index(&components);
    let opponent_notes = opponent_notes(concentration, profitable_share, sell_dominant_share);

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
            methodology: "Transparent diagnostic built only from available GMGN holder/trader observations. It is not a probability or a buy/sell signal.",
            components,
        },
        opponent_notes,
        chain_evidence,
        sources,
    }
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
            "profitable_holder_share" => 0.25,
            "sell_dominant_traders" => 0.30,
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
    concentration: Option<f64>,
    profitable_share: Option<f64>,
    sell_dominant_share: Option<f64>,
) -> Vec<String> {
    let mut notes = Vec::new();

    if let Some(value) = concentration {
        notes.push(format!(
            "The first ten observable holder rows control about {value:.1}% of the returned holder balance."
        ));
    }

    if let Some(value) = profitable_share {
        notes.push(format!(
            "{:.0}% of observable top holders with PnL fields are currently recorded above cost.",
            value * 100.0
        ));
    }

    if let Some(value) = sell_dominant_share {
        notes.push(format!(
            "{:.0}% of observable top traders currently show more sell volume than buy volume.",
            value * 100.0
        ));
    }

    if notes.is_empty() {
        notes.push(
            "GMGN did not return enough normalized holder/trader fields for Water to derive opponent pressure without guessing."
                .to_string(),
        );
    }

    notes
}

fn top_ten_concentration(value: &Value) -> Option<f64> {
    let rows = find_wallet_rows(value)?;
    let values: Vec<f64> = rows
        .iter()
        .take(10)
        .filter_map(|row| {
            row.as_object().and_then(|object| {
                number_from_object(
                    object,
                    &["amount_percentage", "percentage", "holding_percentage", "percent"],
                )
            })
        })
        .collect();

    if values.is_empty() {
        return None;
    }

    let mut total: f64 = values.iter().sum();
    if total <= 1.5 {
        total *= 100.0;
    }

    Some(total.clamp(0.0, 100.0))
}

fn positive_pnl_share(value: &Value) -> Option<f64> {
    let rows = find_wallet_rows(value)?;
    let mut known = 0_u32;
    let mut positive = 0_u32;

    for row in rows.iter().take(20) {
        let Some(object) = row.as_object() else {
            continue;
        };

        let realized = number_from_object(object, &["profit", "realized_profit", "realized_pnl"]);
        let unrealized =
            number_from_object(object, &["unrealized_profit", "unrealized_pnl", "floating_profit"]);

        if realized.is_some() || unrealized.is_some() {
            known += 1;
            if realized.unwrap_or_default() + unrealized.unwrap_or_default() > 0.0 {
                positive += 1;
            }
        }
    }

    (known > 0).then_some(positive as f64 / known as f64)
}

fn sell_dominant_share(value: &Value) -> Option<f64> {
    let rows = find_wallet_rows(value)?;
    let mut known = 0_u32;
    let mut sellers = 0_u32;

    for row in rows.iter().take(20) {
        let Some(object) = row.as_object() else {
            continue;
        };

        let buy = number_from_object(
            object,
            &["buy_volume_cur", "buy_volume", "buy_amount", "buy_volume_usd"],
        );
        let sell = number_from_object(
            object,
            &["sell_volume_cur", "sell_volume", "sell_amount", "sell_volume_usd"],
        );

        if let (Some(buy), Some(sell)) = (buy, sell) {
            known += 1;
            if sell > buy {
                sellers += 1;
            }
        }
    }

    (known > 0).then_some(sellers as f64 / known as f64)
}

fn find_wallet_rows(value: &Value) -> Option<&Vec<Value>> {
    match value {
        Value::Array(rows) => {
            let looks_like_wallet_rows = rows.iter().any(|row| {
                row.as_object().is_some_and(|object| {
                    ["address", "wallet_address", "holder_address", "owner"]
                        .iter()
                        .any(|key| object.contains_key(*key))
                })
            });

            if looks_like_wallet_rows {
                return Some(rows);
            }

            rows.iter().find_map(find_wallet_rows)
        }
        Value::Object(object) => object.values().find_map(find_wallet_rows),
        _ => None,
    }
}

fn find_string(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(object) => {
            for key in keys {
                if let Some(candidate) = object.get(*key).and_then(Value::as_str) {
                    if !candidate.trim().is_empty() {
                        return Some(candidate.to_string());
                    }
                }
            }
            object.values().find_map(|child| find_string(child, keys))
        }
        Value::Array(values) => values.iter().find_map(|child| find_string(child, keys)),
        _ => None,
    }
}

fn find_number(value: &Value, keys: &[&str]) -> Option<f64> {
    match value {
        Value::Object(object) => {
            for key in keys {
                if let Some(candidate) = object.get(*key).and_then(value_as_f64) {
                    return Some(candidate);
                }
            }
            object.values().find_map(|child| find_number(child, keys))
        }
        Value::Array(values) => values.iter().find_map(|child| find_number(child, keys)),
        _ => None,
    }
}

fn number_from_object(
    object: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<f64> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(value_as_f64))
}

fn value_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(value) => value.replace(',', "").parse::<f64>().ok(),
        _ => None,
    }
}
