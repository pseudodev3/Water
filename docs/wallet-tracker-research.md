# Wallet tracker: client audit and data research

Research date: 2026-10-03. Scope: automatic discovery, historical qualification
and continuous observation of trading wallets on Solana and Robinhood mainnet
(chain ID 4663), including Pump.fun, Fomo.family and directly discovered onchain
wallets. Budget: free public access and free keyed plans; no paid subscription.

## Decision

**Do not adopt the FOMOTrade bot or its history-completion flag as Water's
production foundation.** It is useful endpoint research, has substantial tests
and contains thoughtful failure handling, but it is built for activity alerts
and bounded seeding. It does not establish a complete 30/60-day economic record.
Independent replay tests reproduced eight cases where incomplete or invalid
history leaves its pagination-success flag true.

Fomo's own publicly shipped frontend independently confirms the swap cursor
`lastSwapIdV2`, the transfer cursor `lastTransferId`, and `hasNextPage` response
semantics. That is stronger evidence than community comments. It establishes a
client contract, not the server's historical retention, every execution wallet,
or complete SOL/RH coverage. No legitimate Fomo session is configured here, and
no authenticated history was tested.

There is also an operational constraint: Fomo's current terms, section 16,
explicitly prohibit automated collection of transaction/account data. The
community README itself warns about account bans. A normal account login is
therefore insufficient evidence of an approved, dependable Water integration.
Use an expressly permitted integration or permitted export if one becomes
available; keep independent chain collection as the accounting foundation.
No message requesting integration access has been sent.

The recommended implementation is a small Water-owned Rust collector with
strict schemas, explicit coverage results and durable evidence. Pump/Fomo
rankings can nominate candidates where access is permitted and fresh. Water
must calculate qualification from reconciled chain economics rather than
inheriting a provider's profit or verification badge.

## Audit scope and reproducibility

Audited repository: [GakkiYuiMIO/FOMOTrade](https://github.com/GakkiYuiMIO/FOMOTrade).
Pinned commit: `58c8f2f0329798f4a61fba5bfb63a23d397e5c80`, dated 2026-09-26.
The repository was created on 2026-08-11 and had 156 commits, 7 stars and 3 forks
when checked. Recent activity is a positive maintenance signal; its age and
adoption do not establish long-term production reliability. No GitHub Actions
runs were found. The MIT license permits reuse subject to attribution; that
license does not grant access rights to Fomo's service.

Only isolated tests and public, identified, TLS-verified HTTP reads were used.
No real login tokens, private keys, trades or Telegram messages were used.
Browser stealth, Chrome TLS impersonation and WAF evasion were not used for
live research. The whole community application was not installed or started.

| Verification | Result | Interpretation |
| --- | --- | --- |
| Upstream client and poller tests | 200 passed; rerun with pytest/curl versions within declared ranges and native HTTP disabled | Good regression coverage for their existing contracts, not proof of complete wallet economics |
| Full upstream suite in the initial audit environment | 5,010 passed, 5 failed out of 5,015 | Five holder-output tests received an unrelated live Pump panel because its native HTTP transport escaped the Python socket guard |
| Those five tests with the missing Pump HTTP stub | 5 passed | Demonstrates a test-isolation gap; not evidence of five wallet-accounting defects |
| Exact-source pagination replay | 11 synthetic cases; 8 false-success cases, 3 controls | The completion flag is unsuitable for Water qualification |
| Exact first-party frontend function replay | 8 checks passed | Confirms route construction, named cursors, short-page continuation and error-envelope rejection |
| Exact-source session storage replay | File mode 0644 under umask 022; short named fake secret not masked | Credential storage/logging need stronger controls if an approved adapter is ever built |

The full-suite run was not wholly network-isolated. The final focused run denied
native curl requests as well as external Python socket connections. Full-suite
results also came from a different dependency environment; they are reported
as observed, not as a certified lockfile build. Synthetic audit fixtures contain
fictional records and are never inputs to Water's product or performance data.

Evidence summaries: [pagination counterexamples](research/wallet-tracker/pager-counterexamples.json),
[first-party contract checks](research/wallet-tracker/first-party-contract-tests.json),
[free coverage probes](research/wallet-tracker/free-coverage-probes.json),
[RH archive validation](research/wallet-tracker/rh-archive-validation.json),
[session storage checks](research/wallet-tracker/session-storage-audit.json), and
[test environment](research/wallet-tracker/focused-test-environment.json).
The [test summary](research/wallet-tracker/test-summary.json) preserves the
initial broad-run failures alongside the isolated and focused passing runs.

The workspace also retains the reproduction scripts and XML/log outputs under
`/workspace/water-wallet-research`. Community source extracted for the isolated
replay retains the upstream MIT notice in `LICENSE.FOMOTrade`. Fomo's shipped
JavaScript is a local research capture, not copied into Water's implementation.

## What is good in the community client

- Named cursor pagination, repeated-page detection and ID deduplication address
  a real failure mode: an ignored offset can replay page one indefinitely.
- HTTP handling distinguishes authentication failure, transport failure,
  rate limiting and WAF responses. It has bounded attempts, Retry-After handling
  and jitter. These are useful patterns for an approved Water adapter.
- Snapshot components fail independently, so unavailable balances need not
  erase successfully received activity. Optional decoration has a shorter
  failure budget and cannot invalidate the shared authentication session.
- The author documents limits rather than hiding all of them. Examples include
  holder coverage, refresh failures and unsupported browser backfill.

These strengths justify reading the project as a reference. They do not justify
using its ready/completed state as a historical-profitability certificate.

## Reproduced history failures

The following are exact-source, synthetic replays of
[`iter_swap_buys`](https://github.com/GakkiYuiMIO/FOMOTrade/blob/58c8f2f0329798f4a61fba5bfb63a23d397e5c80/src/client.py#L1055).
Its implementation initializes `_last_paging_ok = True`; several early exits
do not change it. The flag is used in the seeding completion message.

| Input/stop condition | Observed behavior | Why Water must reject historical qualification |
| --- | --- | --- |
| Item budget reached while more history exists | Stops with success flag true | The requested window may be missing most trades |
| 20 pages reached while more history exists | Returns 1,000 rows, success flag true | A count cap is not a time or inventory boundary |
| One-row page explicitly says `hasNextPage: true` | Stops after page one, success flag true | Page length overrides the explicit continuation signal |
| Empty page explicitly says `hasNextPage: true` | Returns zero rows, success flag true | An inconsistent response becomes an apparently empty history |
| HTTP 200, `success: false`, statusCode 403 | Returns zero rows, success flag true | Application failure is interpreted as no trades |
| Changed/unrecognized response structure | Returns zero rows, success flag true | Schema failure is interpreted as no trades |
| Non-object swap rows | Silently drops rows, success flag true | Invalid data disappears without an accounting gap |
| Two different fills share a hash and lack stable record IDs | Keeps only one, success flag true | Transaction identity is insufficient for fill identity |

The controls correctly handled a terminal second page, detected a repeated full
page, and accepted a valid empty terminal page. The defects are specific to
completeness, not a claim that every normal request is broken. Some caps are
reasonable for an alert bot; Water's requirement is materially stricter.

Other important scope differences:

- Default seeding is 500 records. Historical seeding writes aggregated buy
  counters rather than preserving the complete raw historical swap ledger
  ([poller](https://github.com/GakkiYuiMIO/FOMOTrade/blob/58c8f2f0329798f4a61fba5bfb63a23d397e5c80/src/poller.py#L2890)).
- The browser transport explicitly does not implement swap backfill
  ([client](https://github.com/GakkiYuiMIO/FOMOTrade/blob/58c8f2f0329798f4a61fba5bfb63a23d397e5c80/src/client.py#L1371)).
- Transfer pagination also uses bounded/short-page stopping; it does not supply
  a wallet-economic completeness certificate.
- Price-history retention defaults to three days, intended for sparklines,
  rather than independently reconstructing portfolio marks over two months.
- Session writes do not enforce restrictive file permissions. Logging has
  heuristic long-string masking, so the finding is not that all real tokens
  leak; it is that neither storage nor structured secret redaction is strong
  enough to inherit unchanged.

## What Fomo's first-party frontend proves

Public asset checked:
[history-v2-ChyXtLDq.js](https://fomo.family/assets/history-v2-ChyXtLDq.js),
SHA-256 `400b40dcb7580b6b993cf89ed9672e465d025a4b561c8af82acb239a2cedb925`.

| History | Route | Continuation |
| --- | --- | --- |
| Swaps | `GET /v2/users/{id}/swaps` | Last swap's `id` as `lastSwapIdV2`; optional `tokenAddress` filter |
| Transfers | `GET /v2/users/{id}/transfers` | Last transfer's `id` as `lastTransferId` |

The shipped functions reject `success: false` before returning
`responseObject`. Their infinite-query continuation uses `hasNextPage`, without
assuming that fewer than 50 rows means completion. This independently confirms
both an endpoint lead and two places where the community implementation needs
stricter handling.

The standard profile history supports continued pagination. A separate replay
helper stops at ten pages and can report `isComplete` after a cap or fetch error;
that is UI completion, not economic completeness. The fetch module uses Privy
access tokens and supported-chain headers. The chain configuration includes RH
4663. None of these facts proves that every user's raw swaps extend 60 days or
include every off-app trade, fee and owned execution account.

An approved integration would still need an acceptance test: unique pages,
canonical IDs, timestamps through the requested boundary, full transfers and
fees, chain-specific execution wallets, opening lots, ending balances and
independent transaction reconciliation. A complete Fomo-app swap list cannot
by itself certify a wallet that also trades elsewhere.

## Free data paths and actual limits

| Source | Established evidence | Useful role / unresolved requirement |
| --- | --- | --- |
| Pump monthly board | Public HTTP 200, addresses, realized/unrealized results and row refresh times | Candidate discovery; provider PnL is not reconciled qualification and merged rows can include other chains |
| Pump shipped user-trades contract | Cursor-paged history contract; currently describes Solana coverage | Useful SOL lead; does not establish complete RH economics |
| PooTracker public Fomo index | Public board/status available; documents partial indexed history | Discovery only; captured 30-day board was stale, last successful collection 2026-09-13 |
| fomoapi.io | Free keyed offering documented; anonymous board request returned 401 | Profile/discovery option; its closed-position history is explicitly incomplete |
| Fomo first-party raw history | Public frontend confirms request/cursor contract | Unapproved automation is not a dependable feed; authenticated historical depth remains untested |
| Solana PublicNode | 51 signatures for a real candidate, subsequent `before` page empty, oldest returned transaction readable | Demonstrates some access; does not distinguish a short actual history from provider retention or cover closed token accounts |
| Helius historical RPC | Documents full transactions, owned-token-account discovery, cursor/date filters and unlimited mainnet retention | Strong free keyed SOL candidate; no local key, access/coverage not acceptance-tested |
| RH official RPC and PublicNode | Official RPC rejected both tested trace methods; PublicNode rejected debug tracing | Standard receipts/logs useful; insufficient evidence for all internal ETH flows |
| RH Blockscout | Production already has a free Pro key; anonymous Pro returned 402, direct explorer routes returned 403 | Validate paged address history and internal transactions with existing configured access; local credential unavailable |
| RH Alchemy | Official chain docs recommend free mainnet access; Alchemy lists Transfers support | Free tier explicitly excludes Debug and Trace; Transfers docs do not list RH internal-transfer support |
| RH dRPC | Returned a September 1 trace with 43 calls and an August 4 trace with 12 calls and native ETH movement; roots matched official chain data | Free historical tracing is demonstrated for two samples; timeouts and an earlier older-trace failure prevent a complete-wallet or service-reliability claim |
| RH Ordofi | Mainnet chain ID confirmed; historical trace failed | Error described roughly 1.2M blocks / 1.5 days of local state and failed older upstream fallback |
| RH Arrow RPC | HTTP 530 during capability probe | No usable capability established |

Public/provider behavior is a dated observation, not a permanent service
guarantee. No WAF challenge was bypassed. Chain IDs were checked before tracing
third-party RH endpoints. The September 1 reference is token-launch activity
with zero native value. A later selected August 4 transaction, around the 60-day
boundary, successfully returned an old trace with native ETH movement, matched
against the official transaction and successful receipt. An earlier old-trace
request failed with method-unavailable, and a standard free request timed out;
retain both failures in the evidence. This improves the free RH path, but does
not establish all-wallet enumeration, throughput or continuous availability.
The old trace contains a DELEGATECALL with inherited value: counting it as
another ETH transfer would double-count economic movement.

### Budget arithmetic

Helius's free plan documents 1M credits/month and 10 RPC requests/second.
`getTransactionsForAddress` meters full transactions at 10 credits per 100
returned, rounded up, minimum 10; failed API responses are free. In an ideal
bulk backfill, 50 wallets with 10,000 returned transactions each cost about
50,000 credits for these reads alone. This excludes ownership discovery outside
the method, metadata, prices, retries on other endpoints and ongoing collection.

Frequent tiny polls are the bigger trap. Ten wallets polled every minute for
30 days at a ten-credit minimum would consume 4.32M credits on this method.
At five-minute intervals they would consume 864,000 before other work. This is
arithmetic under the documented minimum, not a measured production bill.

Use bulk backfills, persisted continuation, bounded cohorts and standard free
subscriptions where supported to wake collection; always reconcile subscription
gaps. Do not assume a wallet-only subscription observes every token-account
transfer. Separate the scanner's budget from wallet collection. Define a hard
quota below the free limit and display the actual observed feed delay.

Alchemy documents 30M free compute units/month and 300 base CU/second. CU limits
are method-dependent and cannot be equated with an unrestricted request rate.
Free archive node reads do not imply free Debug/Trace access. The existing free
Blockscout key should be checked before introducing another dependency.

## Requirements for a trustworthy wallet record

### Identity and discovery

Use `(chain, execution account)` as the ledger identity. Preserve Solana base58
case; normalize EVM hex only. Profiles, signers, fee payers, smart accounts,
routers and custody/program accounts are different roles. Establish account
association from explicit evidence before combining records. Funding proximity
and similar trading do not prove common ownership.

Discover both buyers and sellers from documented venue/time samples, preserve
fully exited wallets, and sample established as well as newly launched tokens.
Today's largest holders and winners alone create survivorship bias. App
membership, Pump-origin token provenance and use of a Pump program are separate
attributes. A chain transaction may establish a program without identifying the
frontend through which it was submitted. Keep unsupported attribution unknown.

### Completeness before profit

Record the requested and actually covered time ranges, continuation state,
opening basis, unresolved records, price/fee coverage, finalized boundary and
ending-balance reconciliation. A successful last HTTP call does not establish
complete history. A cap, schema error, dropped fill, missing transfer or missing
native trace keeps qualification incomplete.

Backfill older than the 60-day boundary when inventory sold inside the window
was acquired earlier. Never start old inventory at zero cost. Persist each raw
source record and canonical transaction/fill ordering; timestamps alone cannot
order same-second executions. Deduplicate by chain, transaction and evidenced
event/fill index, rather than by transaction hash alone.

### Economic reconstruction

- Use actual execution asset deltas and FIFO cost basis. Group partial fills
  and exits into position episodes; thirty sell fills are not thirty wins.
- Separate deposits, withdrawals, bridges, account moves, airdrops, creator
  allocations and creator income from trading gains. Unknown inbound basis
  remains unknown; ownership continuity requires evidence.
- Include open-position losses and the change in unrealized PnL at each window
  boundary. Historical accounting PnL is not necessarily cash realizable at the
  displayed price; label price/liquidity assumptions. Unpriceable inventory
  must remain a qualification gap.
- Include failed-transaction fees, priority fees, tips and app fees where
  evidenced. Avoid adding estimated LP fees or slippage a second time when
  execution deltas already include them. Partition swap costs, rent, fees,
  transfers and refunds so the same asset movement is counted once.
- Convert quotes at evidenced historical times, and disclose estimation and
  passive SOL/ETH exposure separately. A USD gain from a rising quote asset is
  not necessarily repeatable token-selection performance.
- Portfolio return and drawdown require cash-flow-adjusted historical equity.
  Recycled buy spend is not invested capital; a realized-profit curve cannot
  measure equity drawdown while losses remain open.

Solana-specific requirements include owned and closed token accounts, SPL and
Token-2022 behavior, inner instructions, v0 loaded addresses, wrapped SOL,
account rent/refunds and exact fee-payer attribution. Current Water history
explicitly warns that native flow can include rent; it cannot be promoted to a
complete wallet record unchanged.

RH is an Arbitrum-based L2. Official docs say L1 data and L2 execution costs are
bundled into gas; do not add an OP-style extra L1 fee by assumption. ERC-4337 and
EIP-7702 accounts are first-class supported accounts. Attribute the user account,
bundler, sponsor and actual charged costs correctly; excluding all contracts
would exclude legitimate traders. Trace failed/reverted call subtrees correctly
and include only effective asset movement. Sequencer observations are
provisional; apply the documented L1 posting/finality stages and reorg recovery.

Spot qualification must also identify unsupported liabilities, LP/staking or
derivative positions affecting the wallet's economics. Either account for them
or disclose the narrower scope and withhold a wallet-wide certificate.

## Qualification and useful product features

The companion [implementation brief](wallet-tracker.md) defines a proposed
60-day tier with two separately profitable 30-day windows, and a weaker 30-day
tier. Sample size, active days, profit factor, removal of the largest winner and
profit concentration are explicit gates. These initial thresholds are product
hypotheses, not empirically calibrated proof of future skill. Version every gate
and retain rejected candidates so the policy can be assessed honestly.

After complete accounting, prioritize:

1. Wallet detail with both months, every winning/losing episode, open exposure,
   actual holding times, fees and visible qualification/coverage reasons.
2. Entry/addition/partial-exit/full-exit activity with executed amounts,
   transaction evidence and measured observation delay.
3. Token overlap among independently qualified wallets, preserving their
   qualification at the time of observation; coordinated or commonly funded
   addresses do not automatically become independent confirmations.
4. Historical trade-size/liquidity context and current executable liquidity.
   A wallet's observed profit does not demonstrate that another trader could
   obtain the same entry, exit or capacity.
5. Direct funding links, proven creator/control relationships and existing
   scanner concentration/authority evidence, with source age and explicit gaps.

A small set of well-evidenced wallets is preferable to manufacturing an
impressive ranking. An empty qualified list is a valid result. Expire stale
qualification and show pending/incomplete records separately.

## Test whether following wallets adds value

Past 30/60-day winners are only a selection cohort. Freeze qualification at
time T using only evidence available by T, then observe subsequent performance
for a forward paper evaluation. Keep wallets that later lose, stop trading or
exit the universe; do not retrospectively replace them with new winners.

Record alert receipt time, contemporaneous executable quote, available liquidity,
fees and the later exit evidence. Evaluate realistic latency and slippage;
unavailable executable quotes make an opportunity unscored, not a hypothetical
profitable fill. Compare against passive native-asset exposure and a documented
candidate baseline. Show cohort size, gains/losses, capital assumptions and the
evaluation interval. Do not optimize many thresholds on the same evaluation
period and call the selected winner an independent validation.

The first target is a 14–30-day forward observation of a bounded cohort after
retrospective records pass their gates. This is a proposed validation interval,
not a replacement for the required 30/60-day historical qualification. No
automated trading or copy-trading is part of Water's collector.

## Delivery gates

1. Prove free SOL history and RH address/internal-transfer enumeration on real
   wallets across 30/60-day boundaries, including pre-window basis and at least
   one native-asset swap. Check historical balances and independent references.
   Fomo-app association needs permitted evidence; direct chain qualification
   must work independently of a private Fomo session.
2. Establish durable SQLite storage on the existing deployment budget, one
   writer, saved cursors, raw evidence and restart/outage recovery. A Railway
   container's local filesystem alone is not durable. Do not purchase storage
   or data services as a side effect of implementation.
3. Build strict provider adapters and wallet-wide accounting with economic
   regression cases. Missing data must propagate into coverage and every gate.
4. Add automatic discovery, rankings, wallet detail, follow/feed and token
   overlap. Show each chain independently and validate actual mobile flows.
5. Run the forward paper evaluation and publish the measured usefulness and
   feed delay before expanding the cohort or considering any paid data.

Research is complete enough to reject as-is community integration and define
the next engineering gates. Full authenticated SOL/RH coverage, permitted Fomo
data integration and durable tracker deployment remain unverified. No tracker,
ranking or background monitoring was deployed during this research pass.

## Primary sources

- [Community source, pinned](https://github.com/GakkiYuiMIO/FOMOTrade/tree/58c8f2f0329798f4a61fba5bfb63a23d397e5c80),
  [README account-ban warning](https://github.com/GakkiYuiMIO/FOMOTrade/blob/58c8f2f0329798f4a61fba5bfb63a23d397e5c80/README.md#L17),
  [MIT license](https://github.com/GakkiYuiMIO/FOMOTrade/blob/58c8f2f0329798f4a61fba5bfb63a23d397e5c80/LICENSE).
- [Fomo history module](https://fomo.family/assets/history-v2-ChyXtLDq.js),
  [fetch module](https://fomo.family/assets/fomoFetch-v2-dqYf-QA0.js),
  [chain module](https://fomo.family/assets/chains-v2-B4QBB5Lf.js), and
  [Fomo terms, section 16](https://fomo.family/terms).
- [Pump monthly board](https://frontend-api-v3.pump.fun/pnl-leaderboard?period=monthly&sort=realized&limit=5)
  and [Pump public frontend](https://pump.fun/).
- [PooTracker source/docs](https://github.com/deladevsol/fomo-api),
  [collector status](https://fomo-public.pootracker.app/v2/status), and
  [public 30-day board](https://fomo-public.pootracker.app/v2/leaderboards/traders?window=30d).
- [Independent Fomo API documentation](https://fomoapi.io/docs) and
  [free keyed pricing](https://fomoapi.io/pricing).
- [Solana signature history](https://solana.com/docs/rpc/http/getsignaturesforaddress)
  and [transaction metadata](https://solana.com/docs/rpc/json-structures).
- [Helius historical RPC](https://www.helius.dev/docs/rpc/gettransactionsforaddress)
  and [free-plan/method limits](https://www.helius.dev/docs/billing/plans).
- [Robinhood chain documentation](https://docs.robinhood.com/chain),
  [connections](https://docs.robinhood.com/chain/connecting),
  [gas and fees](https://docs.robinhood.com/chain/gas-and-fees),
  [account abstraction](https://docs.robinhood.com/chain/account-abstraction),
  [finality](https://docs.robinhood.com/chain/transaction-finality).
- [Alchemy RH methods](https://www.alchemy.com/docs/robinhood-chain/robinhood-chain-api-overview),
  [free plan vs Debug/Trace](https://www.alchemy.com/docs/reference/pricing-plans),
  [Transfers API internal-transfer limitations](https://www.alchemy.com/docs/reference/alchemy-getassettransfers).
- [Blockscout REST APIs](https://docs.blockscout.com/devs/apis/rest),
  [dRPC RH endpoint](https://drpc.org/chainlist/robinhood),
  [dRPC RH API documentation](https://drpc.org/docs/robinhood-api).

First-party documentation and live responses are separated throughout this
report. Marketing claims, source contracts, mocked tests and authenticated
production coverage are different levels of evidence.
