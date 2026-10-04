# Wallet discovery and tracker — implementation brief

Status: implementation is on `feat/wallet-tracker`; endpoints, collector and UI
exist locally. Production tracker and alerts are not deployed. Research checked
on 2026-10-03. See [implementation and acceptance](wallet-tracker-implementation.md).

Budget constraint: use free public access and free keyed tiers. The user does
not want a $49/month data subscription before the tool has demonstrated value.
No paid data plan is part of the current implementation scope.

The [detailed research and client audit](wallet-tracker-research.md) records
source contracts, reproduced completeness failures, free-provider probes and
the adoption decision. FOMOTrade is a research reference, not the tracker
runtime. Fomo's current terms prohibit automated collection; any direct app
integration requires expressly permitted access. Chain accounting must work
independently of a private Fomo feed.

## Product scope

Automatically discover candidates from Pump.fun, Fomo.family and relevant direct
onchain trading activity, assess their actual execution wallets on Solana and
Robinhood Chain (4663), and continuously follow wallets with repeatable
profitability over at least 30 days. Pump/Fomo are discovery sources and filters,
not an exclusive wallet whitelist. This interprets the user's scope correction
as "shouldn't be limited"; an explicit clarification can narrow it. The preferred
qualification is 60 complete days with positive results in each separate 30-day
window. A profile's popularity or provider verification badge is not performance
verification.

Separate discovery from qualification. A leaderboard can nominate a candidate;
it cannot certify that candidate. Missing, stale or incomplete records remain
unranked, with the reason visible. The discovery universe is bounded and must
be stated; Water cannot claim it has searched every wallet on either chain.

Track chains separately. A merged app leaderboard can contain Ethereum, BSC or
other chains; its aggregate PnL must not be presented as SOL/RH performance.
Combine wallets into a profile only with explicit, sourced account association,
and retain each chain's record. Do not infer common ownership from funding or
similar behavior. A profile address may differ from its trading execution wallet.

Pump-origin tokens, transactions through a Pump program, and activity through
the Pump/Fomo apps are different attributes. Preserve all three when evidenced;
leave app attribution unknown when chain evidence alone cannot establish it.
Recognize supported programs/factories by exact evidence, not ticker or suffix.

## Relevant onchain context

The tracker also needs evidence about what a wallet is doing and the tokens it
touches. Positions, direct funding observations, background feeds and fresh qualifying
holdings overlap are implemented. The scanner supplies market/origin evidence
through position links. Full funding graphs and wallet-to-creator attribution
remain subsequent capabilities.

| Context | Useful observation | Evidence requirement |
| --- | --- | --- |
| Position changes | First entry, additions, partial/full exits, open size and holding time | Reconciled execution/transfer records, with actual size and time |
| Shared tokens | Several qualifying wallets entering, holding or exiting the same asset | Distinct wallet records, timestamps and qualifying status at observation time |
| Funding | Observed funding transfers and their immediate source/destination | Direct transaction evidence; a common funder does not prove common ownership |
| Creator/control relationships | A trader is also the proven deployer, launch creator or active authority | Exact creation transaction/event or authority state; no inferred insider label |
| Token structure | Liquidity, wallet concentration, authorities and supported launch provenance | Existing independent market/chain evidence, with its age and coverage |
| Execution constraints | Actual trade size relative to liquidity, holding time and observed execution cost | Historical market/execution evidence; unknown slippage or market impact stays unknown |

Surface entry/exit changes and shared token exposure first, then funding and
creator relationships in the wallet detail. Keep reputation separate from
wallet size: a whale or early buyer still needs the same complete 30/60-day
economic record before entering the profitable-wallet ranking.

Deduplicate the same chain/wallet when it appears in multiple app feeds.
Connected wallets with unproven ownership remain separate, and shared funding
must not silently multiply or reduce the independent-trader count. Disclose
known account associations without presenting a behavioral cluster as a person.

For direct chain discovery, seed candidates from successful trades
in explicitly supported pools/programs/factories over documented time ranges.
Use both buying and selling participants, retain fully exited wallets, and
sample across newly launched and established liquid tokens. Current-holder
snapshots or today's top-gaining tokens alone introduce survivorship bias.
Filter proven vaults/pools/protocol contracts from a trader ranking while
allowing properly evidenced user execution accounts. Unclassified accounts
remain unclassified. Report the sampled venues, token set, time ranges and gaps;
do not call a bounded candidate search chain-wide coverage.

All three discovery paths feed the same qualification policy. A wallet does not
need a Pump/Fomo account or social profile to qualify. Missing app association
is recorded as unknown, not used as a reason to exclude an otherwise complete
onchain record.

## Qualification policy

These are initial implemented research filters, not statistically calibrated claims
of future profitability. Version the policy and expose every gate and input.

| Requirement | 30-day record | 60-day consistent record |
| --- | --- | --- |
| Historical coverage and age | Entire rolling window, with verified trading before its start | Same, covering both complete months |
| Opening inventory | Proven basis for every sold lot acquired before the window | Same |
| Trading profit | Positive realized trading PnL after known fees | Positive in each consecutive 30-day window |
| Open losses | Complete change in open-position PnL; total trading result positive | Positive total result in each window |
| Sample | At least 30 completed position episodes, 10 distinct tokens, 15 active trading days | Those requirements in each 30-day window |
| Profit factor | At least 1.5, with gains and losses both disclosed | At least 1.5 in each window |
| Dependence on one trade | Still positive after removing the largest winning episode | Same in each window |
| Profit concentration | Largest winner at most 35% of gross positive episode PnL | Same in each window |
| Recent activity | At least one verified trade in the last 7 days | Same |
| Economic completeness | Fees, basis, proceeds and window boundaries complete | Same |

A completed episode is a position opened and later fully closed, after grouping
partial fills and exits. Thirty sell transactions are not thirty independent
successful trades. An unresolved transfer-out cannot be treated as a completed
trade. Count wins, losses and break-even episodes separately; do not require a
high win rate when risk/reward explains profitability.

Report profit factor as gross winning episode PnL / absolute gross losing episode
PnL. A zero loss denominator is undefined, not an infinite quality score. Show
the sample and do not promote an all-win small sample through a fabricated value.

Transfers, deposits, withdrawals, bridging, airdrops, token creation allocations
and creator income are separate from trading profit. Unknown inbound basis is
not zero. Carry basis across a wallet move only with explicit economic ownership
evidence. A swap between two non-quote tokens needs both disposal and acquisition
accounting, without duplicating the transaction's fees.

Use FIFO execution economics, with historical quote conversion and complete
opening lots. Failed transactions can still charge fees and must be included.
Account for execution fees, network/priority fees, tips and applicable L2 fees.
Unknown fee components prevent a fully verified after-fee result.
Avoid counting fees/slippage again when already reflected in actual execution
deltas. On RH, official docs describe L1/L2 costs bundled into gas. Smart-account
gas sponsorship and effective native transfers require exact attribution;
DELEGATECALL inherited value is not an additional ETH movement. On Solana,
partition rent/refunds and wrapped SOL from swap proceeds and fees.

Show realized profit, change in unrealized PnL, open exposure, median trade size,
win rate, average win/loss, holding time, active days, profit concentration and
30/60-day record boundaries. Do not let a wallet realize winners and hide its
open losing positions. Illiquid or unpriced inventory remains a qualification
gap; do not value it at zero without evidence.

Equity drawdown needs historical portfolio marks and cash-flow adjustment. It
must not be inferred from a curve of closed-trade profit. Until that data is
complete, show equity drawdown as unknown and label any separate realized-PnL
drawdown precisely. Return percentages must state their capital denominator;
recycled buy spend is not deposited capital or portfolio return.

Rank only qualifying records. Prefer the 60-day tier, then the weaker month's
profit factor, profit excluding the largest winner, and sample size, with a
stable wallet-address tie break. Display the ordered values instead of inventing
a weighted "smart score". SOL/RH ranks use chain-specific results; any combined
view must show both components and its grouping evidence.

## Experience

- A Wallets view with 30/60-day and SOL/RH filters, source freshness, qualification
  status, verified chain-specific results, and clear exclusion reasons.
- A wallet detail view showing each window, the winning and losing positions,
  current exposure, exact transactions and the economic/coverage ledger, plus
  observed funding and creator/control relationships when proven.
- A followed-wallet activity feed with buys, additions, partial exits and full
  exits. Every event includes chain, token, executed size, timestamp and a chain
  transaction link. Finalized activity is separate from provisional observations.
- A token scan can show which qualifying tracked wallets currently hold or have
  recently entered it, with overlap and position sizes. Three wallets in one coin
  is an observation, not an instruction to buy.
- Relevant onchain context uses the same SOL/RH filters and visible evidence
  freshness. Provider or protocol association is a separate filter from wallet
  profitability, so an app badge cannot stand in for an economic record.

Refreshing a watch page is not background monitoring. Persisted collection and
reconnect gap recovery must work before the UI claims continuous tracking.
Outages preserve the last successful record with its age and pause freshness
claims. Qualification expires when the evidence no longer covers the rolling
window or a refreshed wallet no longer meets the policy.

## Live provider findings

| Source | Observed capability | Limit for this feature |
| --- | --- | --- |
| Pump.fun public monthly leaderboard | HTTP 200; real wallet addresses, reported realized/unrealized PnL, per-row refresh time, window label/start | Discovery only; merged results include other chains and do not prove basis, fees, consistency or a second month |
| Pump.fun public frontend contracts | Cursor-paged user trades and portfolio routes described in shipped code | User-trades contract explicitly says Solana-only coverage today; not evidence of complete RH wallet history |
| PooTracker public Fomo API | HTTP 200 using an identified client; free leaderboard/profile/status routes | Last successful 30-day board collection was 2026-09-13 16:11:04 UTC; index explicitly says partial Fomo history |
| PooTracker indexed PnL | Public documentation says indexed swap cash flow | Excludes off-platform swaps/unknown USD amounts; cash flow is not complete realized trading profit |
| fomoapi.io | Documents 30d/all boards, resolved wallets and available trades | A direct leaderboard request returned HTTP 401 requiring a key; docs say trade history cannot be enumerated completely |
| Fomo.family first-party API | Shipped frontend independently confirms lastSwapIdV2 and lastTransferId, error-envelope checks and hasNextPage continuation | Current terms prohibit automated collection; permitted integration and complete authenticated SOL/RH history remain unverified |
| Water's existing Solana reconstruction | Token-specific bounded history with current-balance reconciliation | At most 250 candidate transactions; not a complete wallet-wide 30/60-day index |
| Water's existing RH reconstruction | Token Transfer logs, receipts, top-level native value and gas | Token-specific; misses internal ETH movements |
| RH public RPC | Both debug_traceTransaction and trace_transaction returned JSON-RPC -32601 | Cannot independently complete native ETH swap economics through these methods |
| Blockscout Pro chain 4663 | Existing deployment has a configured free key; local anonymous address-history request returned HTTP 402 | Address, transfer and internal-transaction pagination/coverage need authenticated acceptance checks |
| Helius getTransactionsForAddress | First-party docs advertise mainnet unlimited retention, date filters, full transactions, cursor pagination and owned-token-account discovery | Candidate optional Solana archive adapter; needs configured access, capability testing and cost measurement; does not cover RH |
| Alchemy RH free tier | Official RH docs recommend free access; Alchemy lists archive Node/Transfers API access | Debug/Trace excluded on free tier; Transfers documentation does not list RH internal-transfer support |
| RH dRPC public RPC | September 1 and August 4 trace samples returned; older native-value trace matched official transaction/receipt evidence | Promising free archive supplement; observed timeout/earlier method failure, full address enumeration and sustained coverage still unverified |

A live Pump board demonstrated why both realized and open results matter: one
row reported approximately $691k realized profit but approximately -$484k total
PnL. Those are provider-reported values, not Water-certified economics.

Do not select a paid provider merely because it says "EVM supported". Confirm
Robinhood **mainnet 4663**, complete address history, internal native transfers,
receipt access and the required retention before spending or integration.

### Access and cost checked on 2026-10-03

- Helius lists a free Solana plan with 1M credits/month and 10 requests/second. Its
  getTransactionsForAddress documentation lists 10 credits per 100 returned
  full transactions, rounded up, with a 10-credit minimum. Test authenticated
  access and actual backfill consumption on the free plan first; advertised
  retention does not replace the wallet coverage checks. This
  provider covers Solana, not Robinhood Chain.
- Robinhood's existing Blockscout key is the first indexed-history option to
  validate. Complete internal-transfer coverage and retention remain untested;
  no additional RH subscription price has been established.
- fomoapi.io lists a free keyed plan with 250,000 credits/month. Endpoint costs
  vary: wallet resolution is more
  expensive than a normal leaderboard read. This can supply discovery/profile
  data, but its documented incomplete trade history cannot certify the full
  60-day onchain record.

Use configured free access and measure history and monitoring consumption.
Stay within free limits and mark unsupported
records incomplete instead of introducing a subscription.
Tiny frequent historical-RPC polls can exhaust the free allowance: ten wallets
at one poll/minute and a ten-credit minimum would use 4.32M credits in 30 days.
Use bulk backfill, bounded cohorts, persisted continuation and gap reconciliation;
show actual collection delay and reserve capacity for the scanner.

### Fomo raw-history route to validate

The independent fomoapi.io provider's incomplete closed-position history does
not establish that Fomo's own raw swap history is impossible to retrieve.
First-party shipped code confirms `GET /v2/users/{id}/swaps` with `lastSwapIdV2`
and `GET /v2/users/{id}/transfers` with `lastTransferId`. It rejects failed
application envelopes and continues according to `hasNextPage`, even on a short
page. The community client adds a 20-page/1,000-record cap and can leave its
success flag true on a cap, short page, malformed data or API failure. Exact
source replay reproduced eight false-success cases. Do not adopt that flag or
the full bot as Water's foundation.

Fomo's terms, section 16, explicitly prohibit automated collection of account
and transaction data. Normal authenticated access alone does not establish a
permitted server integration. If expressly permitted access/export becomes
available, validate unique pages through 30/60-day boundaries, SOL/RH execution
wallets, transfers, fees and independent chain reconciliation. No authenticated
Fomo history was tested here. A complete app swap list alone still cannot prove
all activity outside Fomo. Never request a private key or print/commit a login
token to perform validation.

Free fallback probes: Solana public RPC returned signature listings for a real
Pump candidate, which proves some readable history but not full wallet coverage.
Robinhood PublicNode also returned -32601 for debug_traceTransaction. Anonymous
direct Blockscout address-history/internal-transaction requests returned HTTP
403; the existing backend's free Pro key remains the indexed path to validate.
Those probes do not establish complete 60-day coverage.
Additional RH dRPC probes did return traces from September 1 and August 4,
including old native ETH calls matched against official chain evidence. This
establishes sampled free archive capability; wallet-wide history enumeration,
economic completeness and dependable quota/latency remain to be validated.

Start with a small, explicitly bounded candidate cohort and cache successful
reads. Keep historical backfill and monitoring budgets separate from token
scans. Qualify only records whose full requested window and opening basis are
proven; collection from today builds future coverage but does not backfill the
previous two months.

## Implementation architecture

Keep the Rust service and Next frontend. Provider adapters expose discovery,
execution-wallet resolution, paged historical records, live continuation and
coverage independently. Free sources remain useful for discovery. Optional
indexed/archive adapters supply complete economics where public RPC cannot.

Continuous 60-day monitoring has a concrete storage requirement: raw evidence,
canonical record IDs, cursors, accounting checkpoints and policy versions must
survive a service restart. A single SQLite database on a durable volume is a
reasonable first deployment for one Rust collector process; no queue or separate
service is needed. A local Railway container filesystem is not durable storage.
Validate durability and the single-writer deployment configuration before launch.

Run bounded collectors outside scan handlers. Backfill nominated wallets, then
resume incrementally from persisted chain cursors. Fetch older history until
opening lots are proven, not merely until the 60-day boundary. Preserve both
successful and failed execution evidence and handle reorgs, duplicates and
same-second transaction ordering. A budget/page cap marks history incomplete;
it never silently produces a qualifying wallet.

Each record needs chain, wallet, profile association source, discovery source,
transaction/signature, block/slot and canonical transaction order, event/fill
index, finalized status, time, asset quantities/decimals, quotes and fee evidence.
Keep source captures and conversion timestamps. Asset keys retain Solana base58
case; normalize EVM hex only. Partition identity by chain, including tokens with
the same symbol/address string on different chains.

Each analysis needs requested window boundaries, covered boundaries, opening
basis evidence, pagination completion, unresolved records, price/fee coverage,
ending-balance reconciliation, source freshness, policy version and gate results.
Provider "verified" or "complete" flags cannot override those requirements.

## Delivery and acceptance

1. **Free data access and persistence:** validate free Solana indexed history,
   the existing RH index key and sampled dRPC archive tracing. Fomo association
   requires permitted evidence; its private API must not gate chain accounting.
   Establish durable storage within the existing deployment budget. Verify
   execution wallets and SOL/RH boundaries using real transactions. Measure
   per-wallet backfill time and free-tier credit use before increasing the cohort.
2. **Historical qualification:** reconstruct wallet-wide records, include fully
   exited and losing tokens, prove opening inventory, calculate both months and
   all gates, and independently reconcile a sample against chain evidence.
3. **Tracker UI and collection:** automatic discovery from apps and documented
   onchain venue samples, wallet detail, follow/feed, token overlap and sourced
   context; background collection survives restart and fills outage gaps. No
   qualified wallet is required for the list to be truthful.
4. **Release checks:** accounting regressions for transfers, partial exits,
   opening lots, duplicated fills, failed-fee transactions, missing traces,
   losing open inventory, outlier dependence, stale records and separate chains;
   Rust checks, Next build, real-provider checks, mobile interaction review,
   CI/preview, deployment and fresh production checks.
5. **Forward paper evaluation:** freeze qualification using only past evidence,
   observe a bounded cohort for a proposed 14–30-day interval, retain subsequent
   losers/inactive wallets and evaluate actual alert delay, executable quotes,
   fees and liquidity against a documented baseline. Do not infer useful
   follow-through merely from selecting past winners. No trades are executed.

Implementation has proceeded with public fallbacks, strict qualification gates
and durable local evidence. Production acceptance still needs the free indexed
credentials and deployment volume. Current unauthenticated providers do not
establish complete 60-day coverage on both chains; no real wallet has been
certified in this pass.

## Sources

- [Pump.fun frontend](https://pump.fun/) and its public shipped API contracts.
- [Observed monthly Pump board](https://frontend-api-v3.pump.fun/pnl-leaderboard?period=monthly&sort=realized&limit=5).
- [PooTracker API source/documentation](https://github.com/deladevsol/fomo-api),
  [OpenAPI](https://fomo-public.pootracker.app/openapi.json),
  [collector status](https://fomo-public.pootracker.app/v2/status), and
  [30-day trader board](https://fomo-public.pootracker.app/v2/leaderboards/traders?window=30d).
- [Independent fomoapi.io documentation](https://fomoapi.io/docs): authentication,
  provider-reported flags, trade-history completeness limits and pricing.
- [Third-party Fomo execution-wallet caveat](https://github.com/cvxv666/fomo-robinhood-radar/blob/main/fomo_agent/sources/fomo.py):
  a lead requiring independent account/transaction verification, not an authority
  for assigning wallet ownership.
- [Community raw-swap pagination implementation](https://github.com/GakkiYuiMIO/FOMOTrade/blob/58c8f2f0329798f4a61fba5bfb63a23d397e5c80/src/client.py#L1055):
  exact-source audit found unsafe completeness semantics.
- [Fomo first-party history module](https://fomo.family/assets/history-v2-ChyXtLDq.js)
  and [terms, section 16](https://fomo.family/terms).
- [Helius historical transaction documentation](https://www.helius.dev/docs/rpc/gettransactionsforaddress).
- [Helius pricing](https://www.helius.dev/pricing) and
  [plan documentation](https://www.helius.dev/docs/billing/plans).
- [Independent Fomo API pricing](https://fomoapi.io/pricing).
- [Blockscout API documentation](https://docs.blockscout.com/devs/apis/rest).
- [Robinhood mainnet public RPC](https://rpc.mainnet.chain.robinhood.com).
- [RH fees](https://docs.robinhood.com/chain/gas-and-fees) and
  [account abstraction](https://docs.robinhood.com/chain/account-abstraction).
- [Alchemy free plan exclusions](https://www.alchemy.com/docs/reference/pricing-plans)
  and [RH method support](https://www.alchemy.com/docs/robinhood-chain/robinhood-chain-api-overview).
- [dRPC RH endpoint](https://drpc.org/chainlist/robinhood).
