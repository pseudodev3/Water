# Tracker RPC recovery and Pump social coverage

Checked 2026-10-04. Fix branch: `fix/tracker-rpc-pump`; these changes are not
deployed until this branch is merged. Production already includes BNB wallet
tracking through PR #23.

## Reproduced RH failure

The production research wallet
`0x4c64bac8ac8c9e091d0bd7c592a65d06dcf2f88b` had received indexed records, but
account and balance verification failed. The official RH RPC returned JSON-RPC
`-32000`, with "historical state ... is not available", for wallet bytecode,
token precision and token balance at finalized block `0x4c18eda`.
The free RH dRPC endpoint returned those same-height reads successfully. Both
providers reported chain ID 4663 and the same sampled finalized block hash.
This is unavailable state on the primary provider, not proof of an invalid
wallet, token or Blockscout credential.

Transaction reconstruction already used archive fallback. Execution-account
and ending-balance verification omitted it. Those checks now pin a received
finalized block number and use the existing archive path, preserving that
number across fallback. They never substitute `latest` for unavailable history.
Primary state checks, archive fallbacks and trace access validate their EVM
chain identity. BNB keeps its chain-56 public-node failover.

Errors classify missing historical state, quota/rate errors and contract reverts
without reflecting raw provider messages or credential-bearing URLs. If both
routes fail, evidence stays unknown and qualification stays withheld.

## Solana state routing

Production SOL notes also showed `getTokenAccountsByOwner` HTTP 403. State and
discovery reads were using the public fallback even with Helius configured.
Configured Helius now serves execution-account checks, both SPL token-program
balance reads, supported-program discovery and transaction reads through the
same persisted credit reservation, credential failover and cooldown as indexed
history. Quota responses do not switch around the budget. Without Helius, the
existing public research fallback remains available.

No new required variable or paid provider is introduced. Existing
`HELIUS_API_KEY`/`HELIUS_API_KEYS` and `ROBINHOOD_TRACE_RPC_URL` configure these
paths; RH archive defaults to `https://robinhood.drpc.org`. Provider availability
and complete history remain separate acceptance requirements.

## BNB is in the tracker

BNB participates in automatic discovery, evidence collection, activity,
positions, filters, follows and explorer links. Its bounded discovery verifies
successful finalized Pancake V2 execution, historical runtime and factory
membership. An empty sample is not complete discovery. The current production
cohort had SOL/RH candidates and no BNB nomination during this check.

Public known-token log windows cannot prove complete wallet-wide native,
internal, failed or other-token history. BNB therefore stays under research
until a permitted wallet-wide index and historical economics are reconciled.
It is not a verified 30/60-day winner list. The chain filter now says "All chains".

## Pump's app is broader than its Solana launchpad

The official [mobile site](https://app.pump.fun/) says "Trade coins across any
chain" and describes a social network. The shipped interface at
[pump.fun](https://pump.fun/) explicitly names Robinhood and BNB Chain in its
multichain coverage legend, alongside Base and Ethereum. The
[current terms](https://pump.fun/docs/terms-and-conditions) explicitly list
Solana and Robinhood Chain as supported networks and allow additional supported
networks. This establishes multichain product scope, not a complete public
trade-history or execution-wallet API.

Go.fun's page is a separate bounty/submission product. Its payout leaderboard
and creator wallet associations do not establish trading profits.

Water's current public monthly nomination source is
`https://frontend-api-v3.pump.fun/pnl-leaderboard?period=monthly&sort=realized&limit=20`.
The received 20 rows expose `walletAddress`, SOL-denominated results,
`userId`/profile fields and position `chainId` values. The sample contained
Solana profile wallets and positions tagged 1399811149. It did not establish
an RH/BNB execution-wallet mapping. Previously Water guessed RH for every
`0x` address; that inference is removed. Ambiguous EVM addresses and multichain
positions without an execution-wallet association are not assigned to a chain.
Provider PnL and badges never become Water's qualification evidence.

Discovery notes now disclose the missing permitted multichain social-wallet
feed. SOL monthly nominations continue; RH/BNB use their independent onchain
sources. Pump's sections 6.1 and 21(h) require permitted automated access and
restrict collection/tracking of platform users. No private session, mobile
token, realtime credential or authenticated social history was used.

The remaining integration needs a permitted feed or export carrying the actual
execution wallet, explicit chain (SOL, 4663 or 56), observation time and profile
association. These identities must then be reconciled independently onchain.
We have not implemented or certified a complete Pump social trade-history feed
for all three chains, and user/profile addresses are not economic owners by
default. Fomo access remains separately gated; no Fomo credential was configured
in the observed production status.

## Validation

108 Rust tests passed, including pruned-state recovery at unchanged finalized
height, wrong primary/archive chains, absent finality, unavailable archive,
secret-safe error messages, BNB failover, Helius pause/shared budget routing and
ambiguous Pump identities. The production web build passed.

An isolated probe importing the actual Rust provider modules returned verified
EOA state and received zero balances for all three observed RH token contracts.
This proves the repaired state-read path on that sampled wallet; it does not
resolve every pending transaction, trace, historical price or opening lot.
The four-width browser check uses previously received local BNB research
evidence, not an invented wallet or qualified performance record.

Captured checks: [RPC and Pump evidence](research/wallet-tracker/rpc-pump-checks.json).
