use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Chain {
    Solana,
    Robinhood,
}

impl Chain {
    pub fn gmgn_code(self) -> &'static str {
        match self {
            Self::Solana => "sol",
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
    pub sources: Vec<SourceStatus>,
}
