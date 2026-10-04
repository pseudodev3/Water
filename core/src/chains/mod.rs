pub mod robinhood;
pub mod bnb;
pub mod solana;

use crate::{
    config::Config,
    model::{Chain, ChainEvidence, HolderEvidence},
};

pub async fn observe_asset(
    http: &reqwest::Client,
    config: &Config,
    chain: Chain,
    address: &str,
) -> (ChainEvidence, HolderEvidence) {
    match chain {
        Chain::Solana => {
            solana::observe(
                http,
                &config.solana_rpc_url,
                &config.solana_fallback_rpc_url,
                address,
            )
            .await
        },
        Chain::Robinhood => {
            robinhood::observe(
                http,
                &config.robinhood_rpc_url,
                &config.blockscout_api_url,
                config.blockscout_api_key.as_deref(),
                address,
            )
            .await
        }
        Chain::Bnb => bnb::observe(http, &config.bnb_rpc_url, &config.bnb_fallback_rpc_url, address).await,
    }
}
