pub mod robinhood;
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
        Chain::Solana => solana::observe(http, &config.solana_rpc_url, address).await,
        Chain::Robinhood => {
            robinhood::observe(
                http,
                &config.robinhood_rpc_url,
                &config.robinhood_blockscout_url,
                address,
            )
            .await
        }
    }
}
