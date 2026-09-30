pub mod robinhood;
pub mod solana;

use crate::{
    config::Config,
    model::{Chain, ChainEvidence},
};

pub async fn verify_asset(
    http: &reqwest::Client,
    config: &Config,
    chain: Chain,
    address: &str,
) -> ChainEvidence {
    match chain {
        Chain::Solana => solana::verify(http, &config.solana_rpc_url, address).await,
        Chain::Robinhood => robinhood::verify(http, &config.robinhood_rpc_url, address).await,
    }
}
