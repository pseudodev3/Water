use crate::{
    config::Config,
    early::{summarize_cohort, CohortSummary},
    history::{robinhood::RobinhoodHistoryClient, solana::SolanaHistoryClient, HolderCandidate},
    model::Chain,
    position::{analyze_wallet_position, WalletPositionRequest, WalletPositionResponse},
    providers::gecko::GeckoClient,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

const DEFAULT_COHORT_SIZE: usize = 5;
const MAX_COHORT_SIZE: usize = 5;

#[derive(Clone, Debug, Deserialize)]
pub struct HolderCohortRequest {
    pub chain: Chain,
    pub token: String,
    pub limit: Option<usize>,
    pub launch_timestamp: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HolderCohortMember {
    pub source_rank: usize,
    pub source_current_quantity: rust_decimal::Decimal,
    pub analysis: WalletPositionResponse,
}

#[derive(Clone, Debug, Serialize)]
pub struct HolderCohortResponse {
    pub chain: Chain,
    pub token: String,
    pub definition: String,
    pub candidate_source: String,
    pub members_requested: usize,
    pub members_analyzed: usize,
    pub members: Vec<HolderCohortMember>,
    pub cohort: CohortSummary,
    pub notes: Vec<String>,
}

pub async fn analyze_holder_cohort(
    http: Client,
    config: &Config,
    gecko: &GeckoClient,
    request: HolderCohortRequest,
) -> Result<HolderCohortResponse, String> {
    validate_token(request.chain, request.token.trim())?;

    let limit = request
        .limit
        .unwrap_or(DEFAULT_COHORT_SIZE)
        .clamp(1, MAX_COHORT_SIZE);
    let token = request.token.trim().to_string();

    let (candidates, candidate_source, mut notes) =
        holder_candidates(http.clone(), config, request.chain, &token, limit).await?;

    let members_requested = candidates.len();
    let mut members = Vec::new();

    for candidate in candidates {
        match analyze_candidate(
            http.clone(),
            config,
            gecko,
            request.chain,
            &token,
            request.launch_timestamp,
            candidate,
        )
        .await
        {
            Ok(member) => members.push(member),
            Err(error) => notes.push(error),
        }
    }

    // "Early" is deliberately an observed ordering, not a hidden score.
    members.sort_by(|left, right| {
        left.analysis
            .behavior
            .first_acquired_at
            .unwrap_or(u64::MAX)
            .cmp(
                &right
                    .analysis
                    .behavior
                    .first_acquired_at
                    .unwrap_or(u64::MAX),
            )
    });

    let positions: Vec<_> = members
        .iter()
        .map(|member| member.analysis.position.clone())
        .collect();
    let cohort = summarize_cohort(&positions);

    Ok(HolderCohortResponse {
        chain: request.chain,
        token,
        definition: "Current top wallet-controlled holders, with program/PDA/contract-controlled balances excluded, ordered by each wallet's earliest acquisition Water can observe. This is not the first-N historical buyers and does not include wallets that already exited completely.".to_string(),
        candidate_source,
        members_requested,
        members_analyzed: members.len(),
        members,
        cohort,
        notes,
    })
}

pub async fn holder_candidates(
    http: Client,
    config: &Config,
    chain: Chain,
    token: &str,
    limit: usize,
) -> Result<(Vec<HolderCandidate>, String, Vec<String>), String> {
    validate_token(chain, token)?;

    match chain {
        Chain::Solana => {
            let client = SolanaHistoryClient::with_fallback(
                http,
                config.solana_rpc_url.clone(),
                config.solana_fallback_rpc_url.clone(),
            );
            let candidates = client.top_current_holders(token, limit).await?;
            Ok((
                candidates,
                "Solana token accounts grouped by controlling authority; off-curve/program-controlled authorities excluded".to_string(),
                vec![
                    "The candidate set is wallet-only within the holder reconstruction Water could prove; protocol/PDA-controlled balances are excluded."
                        .to_string(),
                ],
            ))
        }
        Chain::Robinhood => {
            let client = match config.blockscout_api_key.as_ref() {
                Some(key) => RobinhoodHistoryClient::with_holder_index(
                    http,
                    config.robinhood_rpc_url.clone(),
                    config.blockscout_api_url.clone(),
                    key.clone(),
                ),
                None => RobinhoodHistoryClient::new(
                    http,
                    config.robinhood_rpc_url.clone(),
                ),
            };
            let candidates = client.top_current_holders(token, limit).await?;
            Ok((
                candidates,
                "Robinhood indexed ERC-20 holders; contract-controlled balances excluded".to_string(),
                Vec::new(),
            ))
        }
    }
}

async fn analyze_candidate(
    http: Client,
    config: &Config,
    gecko: &GeckoClient,
    chain: Chain,
    token: &str,
    launch_timestamp: Option<u64>,
    candidate: HolderCandidate,
) -> Result<HolderCohortMember, String> {
    let wallet = candidate.wallet.clone();
    let analysis = analyze_wallet_position(
        http,
        config,
        gecko,
        WalletPositionRequest {
            chain,
            token: token.to_string(),
            wallet: candidate.wallet,
            launch_timestamp,
        },
    )
    .await
    .map_err(|error| format!("Could not analyze holder {wallet}: {error}"))?;

    Ok(HolderCohortMember {
        source_rank: candidate.source_rank,
        source_current_quantity: candidate.current_quantity,
        analysis,
    })
}

fn validate_token(chain: Chain, value: &str) -> Result<(), String> {
    match chain {
        Chain::Solana => {
            if !(32..=44).contains(&value.len()) {
                return Err("token must be a valid Solana base58 address.".to_string());
            }
            const BASE58: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
            if !value.chars().all(|character| BASE58.contains(character)) {
                return Err("token contains invalid Solana base58 characters.".to_string());
            }
        }
        Chain::Robinhood => {
            if value.len() != 42
                || !value.starts_with("0x")
                || !value[2..].chars().all(|character| character.is_ascii_hexdigit())
            {
                return Err("token must be a 42-character EVM 0x address.".to_string());
            }
        }
    }

    Ok(())
}
