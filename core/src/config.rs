#[derive(Clone, Debug)]
pub struct Config {
    pub port: u16,
    pub gecko_api_host: String,
    pub dexscreener_api_host: String,
    pub solana_rpc_url: String,
    pub solana_fallback_rpc_url: String,
    pub robinhood_rpc_url: String,
    pub blockscout_api_url: String,
    pub blockscout_api_key: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            port: std::env::var("PORT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(8080),
            gecko_api_host: std::env::var("GECKOTERMINAL_API_HOST")
                .unwrap_or_else(|_| "https://api.geckoterminal.com/api/v2".to_string()),
            dexscreener_api_host: std::env::var("DEXSCREENER_API_HOST")
                .unwrap_or_else(|_| "https://api.dexscreener.com".to_string()),
            solana_rpc_url: std::env::var("SOLANA_RPC_URL")
                .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
            solana_fallback_rpc_url: std::env::var("SOLANA_FALLBACK_RPC_URL")
                .unwrap_or_else(|_| "https://solana-rpc.publicnode.com".to_string()),
            robinhood_rpc_url: std::env::var("ROBINHOOD_RPC_URL")
                .unwrap_or_else(|_| "https://rpc.mainnet.chain.robinhood.com".to_string()),
            blockscout_api_url: std::env::var("BLOCKSCOUT_API_URL")
                .unwrap_or_else(|_| "https://api.blockscout.com/4663/api/v2".to_string()),
            blockscout_api_key: std::env::var("BLOCKSCOUT_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty()),
        }
    }
}
