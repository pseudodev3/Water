use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    Solana,
    Robinhood,
}

impl Chain {
    pub fn market_network(self) -> &'static str {
        match self {
            Self::Solana => "solana",
            Self::Robinhood => "robinhood",
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
                    return Err("Solana token address contains invalid base58 characters.".to_string());
                }
            }
            Chain::Robinhood => {
                if address.len() != 42 || !address.starts_with("0x") {
                    return Err("Robinhood token contracts must be a 42-character 0x address.".to_string());
                }
                if !address[2..].chars().all(|character| character.is_ascii_hexdigit()) {
                    return Err("Robinhood token contract contains non-hex characters.".to_string());
                }
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct MarketSnapshot {
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub price_usd: Option<f64>,
    pub liquidity_usd: Option<f64>,
    pub market_cap_usd: Option<f64>,
    pub fdv_usd: Option<f64>,
    pub volume_h24_usd: Option<f64>,
    pub buys_h1: Option<u64>,
    pub sells_h1: Option<u64>,
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
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub price_usd: Option<f64>,
    pub liquidity_usd: Option<f64>,
    pub market_cap_usd: Option<f64>,
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

#[derive(Debug, Serialize)]
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
pub struct ScanResponse {
    pub chain: Chain,
    pub address: String,
    pub scanned_at_unix: u64,
    pub token: TokenSnapshot,
    pub pressure: PressureDiagnostic,
    pub opponent_notes: Vec<String>,
    pub chain_evidence: ChainEvidence,
    pub holder_evidence: HolderEvidence,
    pub sources: Vec<SourceStatus>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
