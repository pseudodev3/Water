#[derive(Clone, Debug)]
pub struct Config {
    pub port: u16,
    pub allowed_origin: String,
    pub gmgn_api_key: Option<String>,
    pub gmgn_api_host: String,
    pub solana_rpc_url: String,
    pub robinhood_rpc_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            port: std::env::var("PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(8080),
            allowed_origin: std::env::var("WATER_ALLOWED_ORIGIN")
                .unwrap_or_else(|_| "http://localhost:3000".to_string()),
            gmgn_api_key: std::env::var("GMGN_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            gmgn_api_host: std::env::var("GMGN_API_HOST")
                .unwrap_or_else(|_| "https://openapi.gmgn.ai".to_string()),
            solana_rpc_url: std::env::var("SOLANA_RPC_URL")
                .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
            robinhood_rpc_url: std::env::var("ROBINHOOD_RPC_URL")
                .unwrap_or_else(|_| "https://rpc.mainnet.chain.robinhood.com".to_string()),
        }
    }
}
