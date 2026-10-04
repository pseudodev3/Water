# Wallet tracker implementation and acceptance

Implementation branch: `feat/wallet-tracker`. Production remains the Gecko
recovery baseline `fbefa2a`; this feature has not been deployed.

## Implemented

- One background collector in the existing Rust service, bounded automatic
  discovery, SQLite evidence, atomic cursor/reference commits, fair wallet
  scheduling, restart persistence, failed-record retry and daily request limits.
- Pump monthly nominations, optional independent Fomo account associations,
  successful signed Solana execution samples and successful finalized RH V3
  and verified BNB Pancake V2 execution samples. Boards never provide Water's PnL. Chain records stay
  separate and identities retain case-sensitive Solana addresses.
- Indexed Helius history with explicit terminal cursors; public Solana research
  fallback. RH transactions, token transfers and internal transactions each
  retain their own continuation. Every route must bridge a gap between polls;
  new gaps queue while older gaps are being filled.
- Canonical order, finality, exact asset quantities, fee-payer checks, FIFO basis,
  completed episodes, both months, age, open losses, sample/PF/outlier gates and
  stale demotion. Unsupported economics withhold aggregate totals. Quote-to-quote
  trades require another ledger adapter and currently block qualification;
  exact native ETH/WETH and BNB/WBNB wrapping is neutral.
- Qualification also verifies the execution account: Solana system wallets and
  RH externally owned accounts or valid EIP-7702 delegations. Program/contract
  accounts need a supported ownership and fee adapter before they can rank.
- Monthly realized results sum each month's individual FIFO disposals. Unknown
  historical USD prices on positions closed before the window do not poison
  later known rounds, including reuse of the same token. Unknown opening lots,
  current sales, fees or boundary valuations still withhold the affected result.
- SOL ownership/rent/native wrapping reconciliation, instruction evidence scoped
  to invoked supported programs and immediate native-transfer counterparties.
- RH native call tracing excludes inherited DELEGATECALL values and reverted
  subtrees. Arbitrum fee payment minus refund must match bundled receipt gas fees
  when system-transfer arrays are present. Sponsored/unresolved account fees
  block qualification.
- RH V3 events require a Sourcify-verified runtime template, documented immutable
  masking, signed opposite pool flows and matching pool tokens at the historical
  block. A forged event topic or modified opcode cannot establish execution.
  This verifies compatible execution semantics, not frontend/app attribution.
- Responsive wallet view, separate qualified/research/following lists, immediate
  filters, detail, persisted browser follows, observed activity and independent
  token overlap. Full positions are loaded only in detail and displayed in
  batches, with open positions first. Existing token scans provide deeper market
  and origin evidence through prefilled position links.
  Opening detail brings it into view and closing returns focus to its row;
  reduced motion and unavailable browser storage are handled explicitly.

BNB chain-56 market/supply scans and observed wallet activity are described in
[Helius budgets and BNB scope](wallet-tracker-bnb.md). BNB public known-token log
windows retain empty-range continuations and received partial receipts. They
cannot certify wallet-wide native/internal/failed coverage; BNB stays under
research. Only the pinned Pancake V2 runtime plus historical token/factory
membership establishes supported BNB swap execution.

Supported SOL instructions: Pump bonding-curve buy/sell and specified Jupiter
V6 route variants. Other Solana programs, PumpSwap-specific adapters, arbitrary
RH pools, token-to-token trades, transfer-basis carry and creator/funding graphs
need additional evidence adapters. Unknown execution semantics remain research
records. An association or common counterparty never merges economic ownership.

## Live evidence

The archived RH native swap used for regression is
`0x3150b403901c6380240fa421549bde4abe6abb810fd4ddbfc979cafcfcfbd736`
from August 4, 2026. Fresh transaction, receipt and block reads matched the
previous research capture. The official RPC rejected historical `eth_getCode`;
free dRPC returned the same-block runtime and decimals, plus a call trace.
Effective native input was exactly `0.0003 ETH`; inherited delegate value was
excluded. Fee payment minus refund and `gasUsed * effectiveGasPrice` both equal
`2,838,598,686,000 wei`. The emitting V3 pool's masked runtime matches Sourcify's
verified source. These sampled facts do not prove address-wide archive coverage
or ongoing free-provider reliability.

Live automatic discovery returned Pump and independently sampled Solana
candidates. Public history remains explicitly incomplete. Anonymous RH address
history returned HTTP 402; no index key is available in this workspace. The
public nomination endpoint rejects mutation by default. Missing storage returns
a paused collector, empty research list and no invented rank.

## Setup still required for production acceptance

1. Mount durable storage within the existing deployment budget and set
   `WATER_TRACKER_DB_PATH`; verify stop/start/redeploy persistence. A configured
   path by itself does not prove the Railway volume exists. Run one collector
   instance per database; multiple replicas do not share an atomic request quota.
2. Configure a free Helius server key, then validate actual full-wallet pagination,
   canonical indices, historical token ownership, opening lots and 30/60-day
   price/fee coverage. Pre-slot 111491819 ownership discovery stays incomplete.
3. Validate all RH index routes using the existing free Blockscout key. Independently
   reconcile their terminal cursors, historic native traces and ending balances
   on at least one wallet. Preserve failures/timeouts as gaps.
4. Optional Fomo discovery needs a permitted independent API key or another
   expressly permitted feed. No login/session tokens are requested or used.
5. Keep BNB research-only until a permitted free wallet-wide index and native
   archive economics are reconciled. Public known-token logs are insufficient.
6. Measure backfill time, disk growth, provider credits and polling gaps before
   expanding the cohort. No paid subscription is configured. After deployment,
   evaluate a fixed cohort forward for 14–30 days, retaining later losers and
   inactive wallets, before claiming useful follow-through.

No real wallet has been certified for two complete months during local acceptance.
Browser checks use received candidate evidence; injected failures are test-only.
Tests construct synthetic economics only inside the test module, never as app data.

## Local validation (2026-10-04)

- `cargo test`: 100 passed. The BNB/credit pass adds shared persisted credit limits,
  empty block-range continuity, chain separation, exact runtime rejection,
  incomplete BNB economics and receipt retention during unavailable archive reads.
  Regression cases cover
  same-asset unknown closed prehistory, unknown opening basis, partial exits,
  failed fees, open losses, withdrawals, protocol accounts, provisional records,
  paging gaps, persistence, scoped instruction logs and archived RH execution.
- `npm run build`: passed with TypeScript checks and `/wallets` prerendering.
- 15 browser checks passed at 320, 390, 768 and 1440px using the production build.
  No horizontal overflow or uncaught browser errors occurred. Checks include
  filters, real detail/gates, reduced-motion scrolling/focus, follows after reload,
  blocked storage, failed-refresh recovery, paused collection and scanner links.
  BNB-specific checks additionally cover all three mobile chain controls, actual
  BNB scan/detail/activity, research gates, explorer links, follows and the
  explicitly labeled shared Helius estimate. Counts and results are recorded in
  the BNB/credit acceptance evidence below.
- A real stop/start retained 1,476 transaction references and two continuations;
  the same-day request count continued from 110 to 164, without false ranks.
  This verifies local restart persistence, not a deployed Railway volume.
- The existing SOL and RH scanner paths still returned received market and
  direct-chain evidence while the collector was active. RH holder indexing
  remains unavailable locally without its key.

The baseline results are recorded in
[`research/wallet-tracker/local-implementation-checks.json`](research/wallet-tracker/local-implementation-checks.json).
The subsequent BNB/credit pass is recorded in
[`research/wallet-tracker/bnb-and-credit-checks.json`](research/wallet-tracker/bnb-and-credit-checks.json).

Push delivery, paper-execution evaluation, creator relationships and a broad
funding graph are subsequent capabilities, not shipped claims.
