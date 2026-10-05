use serde::{Deserialize, Serialize};

/// Received token identity and a current market mark. Never a historical fill.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TokenQuote {
    pub asset: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: Option<u32>,
    pub price_usd: Option<rust_decimal::Decimal>,
    pub observed_at: u64,
    pub source: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    Solana,
    Robinhood,
    Bnb,
}

impl Chain {
    pub fn market_network(self) -> &'static str {
        match self {
            Self::Solana => "solana",
            Self::Robinhood => "robinhood",
            Self::Bnb => "bsc",
        }
    }

    /// Storage/API identity is separate from market-provider network slugs.
    pub fn key(self) -> &'static str {
        match self {
            Self::Bnb => "bnb",
            _ => self.market_network(),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Solana => "Solana",
            Self::Robinhood => "Robinhood",
            Self::Bnb => "BNB Chain",
        }
    }
    pub fn evm_chain_id(self) -> Option<u64> {
        match self {
            Self::Solana => None,
            Self::Robinhood => Some(4663),
            Self::Bnb => Some(56),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ScanRequest {
    pub chain: Chain,
    pub address: String,
}

impl ScanRequest {
    pub fn validate(&self) -> Result<(), String> {
        let address = self.address.trim();

        match self.chain {
            Chain::Solana => {
                if !(32..=44).contains(&address.len()) {
                    return Err("Solana token addresses must be 32 to 44 characters.".to_string());
                }
                const BASE58: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
                if !address.chars().all(|character| BASE58.contains(character)) {
                    return Err(
                        "Solana token address contains invalid base58 characters.".to_string()
                    );
                }
            }
            Chain::Robinhood | Chain::Bnb => {
                if address.len() != 42 || !address.starts_with("0x") {
                    return Err(format!(
                        "{} token contracts must be a 42-character 0x address.",
                        self.chain.label()
                    ));
                }
                if !address[2..]
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
                {
                    return Err(format!(
                        "{} token contract contains non-hex characters.",
                        self.chain.label()
                    ));
                }
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct MarketSnapshot {
    pub basis: Option<String>,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub price_usd: Option<f64>,
    pub liquidity_usd: Option<f64>,
    pub market_cap_usd: Option<f64>,
    pub fdv_usd: Option<f64>,
    pub volume_h24_usd: Option<f64>,
    pub buys_h1: Option<u64>,
    pub sells_h1: Option<u64>,
    pub buyers_h1: Option<u64>,
    pub sellers_h1: Option<u64>,
    pub buyers_h24: Option<u64>,
    pub sellers_h24: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HolderEvidence {
    pub top_ten_percentage: Option<f64>,
    /// Human-unit token supply when chain-native evidence can prove it.
    pub total_supply: Option<f64>,
    pub source: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct TokenSnapshot {
    pub market_data_basis: Option<String>,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub price_usd: Option<f64>,
    pub liquidity_usd: Option<f64>,
    pub market_cap_usd: Option<f64>,
    pub market_cap_basis: &'static str,
}

#[derive(Debug, Serialize)]
pub struct PressureComponent {
    pub key: &'static str,
    pub label: &'static str,
    pub observed: String,
    pub pressure: f64,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct PressureDiagnostic {
    pub index: Option<u8>,
    pub methodology: &'static str,
    pub components: Vec<PressureComponent>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceStatus {
    pub source: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct ChainEvidence {
    pub source: String,
    pub verified: bool,
    pub chain_id: Option<u64>,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct DemandEvidence {
    pub buys_h1: Option<u64>,
    pub sells_h1: Option<u64>,
    pub buy_share_h1: Option<f64>,
    pub transactions_h1: Option<u64>,
    pub unique_buyers_h1: Option<u64>,
    pub unique_sellers_h1: Option<u64>,
    pub buyer_arrival_vs_h24_hourly: Option<f64>,
    pub volume_h24_usd: Option<f64>,
    pub volume_to_liquidity: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct ScanResponse {
    pub chain: Chain,
    pub address: String,
    pub scanned_at_unix: u64,
    pub token: TokenSnapshot,
    pub pressure: PressureDiagnostic,
    pub opponent_notes: Vec<String>,
    pub demand: DemandEvidence,
    pub chain_evidence: ChainEvidence,
    pub holder_evidence: HolderEvidence,
    pub sources: Vec<SourceStatus>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bnb_api_identity_and_market_slug_are_separate() {
        let request = ScanRequest {
            chain: Chain::Bnb,
            address: "0xbb4cdb9cbd36b01bd1cbaebf2de08d9173bc095c".into(),
        };
        assert!(request.validate().is_ok());
        assert_eq!(request.chain.key(), "bnb");
        assert_eq!(request.chain.market_network(), "bsc");
        assert_eq!(request.chain.evm_chain_id(), Some(56));
        assert_eq!(serde_json::to_string(&request.chain).unwrap(), "\"bnb\"");
    }

    #[test]
    fn validates_robinhood_hex_contracts() {
        let good = ScanRequest {
            chain: Chain::Robinhood,
            address: "0x1111111111111111111111111111111111111111".to_string(),
        };
        let bad = ScanRequest {
            chain: Chain::Robinhood,
            address: "0xnot-a-contract".to_string(),
        };

        assert!(good.validate().is_ok());
        assert!(bad.validate().is_err());
    }

    #[test]
    fn rejects_invalid_solana_addresses() {
        let short = ScanRequest {
            chain: Chain::Solana,
            address: "short".to_string(),
        };
        let invalid = ScanRequest {
            chain: Chain::Solana,
            address: "O0Il11111111111111111111111111111111".to_string(),
        };

        assert!(short.validate().is_err());
        assert!(invalid.validate().is_err());
    }
}
