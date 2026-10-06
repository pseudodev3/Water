# Water — Project Handoff

_Last updated: 2026-10-06_

This file is the canonical pickup point for a new ChatGPT/Codex session.

## 0. Start here

Repository: `pseudodev3/Water`

Production:
- Frontend: `https://web-water.vercel.app`
- Backend: `https://water-production-822e.up.railway.app`

Previously verified production baseline:
- `b800369608d4916ab2e52fc0e69491534d85aa3a` (PR #31); main CI, Vercel,
  Railway, live API and responsive browser checks passed.
- PR #26's research UI, PR #27's bounded accounting/runtime diagnostics and
  PR #28's zero-balance Positions filter are merged. All deployments passed.
  The last PR #28 live check returned 122 positive positions, no zero rows and
  4,333 saved records on the sampled wallet.
- Earlier Railway crashes remain unexplained without exit/resource details.
  See [runtime investigation](docs/wallet-runtime-investigation.md).

PR #29 is merged and both Vercel and Railway deployments passed. The default wallet
Positions view now shows only positive quantities with a fresh positive USD
valuation. Unpriced/zero-value positive balances can be inspected with the
checkbox; missing pricing is not called zero value. Counts use the same mark
freshness and received-balance precedence. All closed trades, losses, basis,
fees, source records and qualification gates remain in accounting.

Activity filters complete token-only transfers without a received value before
pagination. Trades, native/stablecoin funding, failed/provisional/unresolved and
fee-only executions remain visible. `include_unvalued` restores all saved rows;
raw source lookup bypasses filtering. Continuations include filter membership.
The screenshot's advertising mint was reproduced and disappears by default.

Scanner metadata now renders the returned image and every supported social link.
GeckoTerminal stays primary; missing fields can use the exact-chain/base-token
Dexscreener fallback. Metadata has a separate bounded, coalesced cache and does
not block scans. No metadata or USD value is invented when both sources fail.
Validation: 132 Rust tests, production web build, nine browser cases at
320/390/1440px, real metadata reads on all three chains, and induced Gecko
failure with live Dexscreener metadata passed. No new variables, paid service,
provider collection allocation or storage migration is required.
Read [wallet visibility and token metadata](docs/wallet-visibility-metadata.md).

PR #30 is merged; Vercel and Railway deployments passed. Holding totals now stand out
from token quantities and per-token prices, with explicit USD labels. Totals use
cents for ordinary amounts and significant/scientific precision for tiny positive
values instead of rounding them to zero. Positions default to highest USD value;
lowest value and recent activity are also available. Missing values sort last in
both value directions. Sorting covers all holdings before the 20-row display limit.

The live evidence check at 21:53 UTC still had no qualified wallets. Background
history/discovery was paused at 2,000 requests, resetting at 00:00 UTC (01:00
Africa/Lagos); current checks retained their separate allocation. Backfill,
economic reconstruction, historical prices and balance reconciliation remain
distinct blockers. One small Solana wallet had complete indexed history but
unresolved economics; that is not complete qualification evidence. RH also
reported an `Invalid hex amount.` state-read error on one account. Current BNB
public token logs cannot prove whole-wallet history; waiting does not fix that
coverage limitation. There is no measured completion ETA.
Read [holding values and evidence readiness](docs/wallet-values-evidence.md).
Validation passed: 132 Rust tests, the Next production build, received-data
browser checks at 320/390/1440px, explicit tiny/zero/missing value cases, all sort
directions before pagination, search/show-more and a phone accessibility check.
PR #30 live browser verification received 1,418 valued holdings on 6HJet,
verified both USD sort directions, and passed without JavaScript errors or overflow.
Health returned HTTP 200 with CORS `*`.

PR #31 is merged and verified in production:
[PR #31](https://github.com/pseudodev3/Water/pull/31).
- Default expensive collection floor: **$1,000 native + wallet token holdings on
  the selected chain**, confirmed with fresh received balances and USD marks.
  A sufficient partial lower bound admits; missing prices or incomplete EVM
  inventories never prove a low total. No profitability gates are relaxed.
- Confirmed low wallets sleep six hours; unknowns receive bounded hourly checks.
  Native-only sufficient holdings avoid token census requests. History,
  transaction reconstruction and historical pricing are gated; source records,
  losses and cursors are retained. Market checks can wake newly eligible wallets.
- The active cohort stays bounded (default 12, highest established value first);
  discovery can use a screening pool up to four times that size. This is a finite
  sample, not exhaustive wallet discovery. BNB/RH token census remains partial.
- UI research uses actual Fomo public pages and official app screenshots: quiet
  dark surfaces, prominent USD values, identity/value rows, and bottom tool
  navigation on phones. **Value checks** exposes pending and low-value wallets;
  default discovery shows only holdings that meet the floor. Positions sorts,
  raw activity inspection, scanner socials/images and strict 30/60-day evidence
  remain available.
- No required configuration, paid provider or schema migration is added.
  Optional `WATER_TRACKER_MIN_WALLET_USD=1000` defaults automatically; `0` disables
  this gate. Keep the existing Railway volume and shared budgets.
- Validation: 137 Rust tests, web production build, controlled eligibility states
  over received evidence at 320/390/768/1440px, follows, profile/holdings sort,
  keyboard tabs, safe empty rollout, and WCAG axe checks with zero violations.
  Screenshots use controlled eligibility values and are not live wallet totals.
Read [capital screening and design evidence](docs/wallet-capital-design.md).
The verified rollout retained all 12 wallets and their saved source records;
three met the minimum through received partial lower bounds. Qualification
was still incomplete. RPC limits and first value checks remained explicit.

Latest follow-up (`feat/followed-wallet-names`):
- Personal names for followed wallets, scoped by chain + execution address.
  The existing `water:followed-wallets:v1` array is retained. Names use the separate
  `water:wallet-names:v1` browser-local map, appear in list/profile/Following
  activity and are searchable. Blank names restore address display; unfollowing
  retains the saved name for refollowing. Addresses, explorers and evidence stay
  canonical. Native dialog supports save, cancel and Escape; blocked storage
  leaves the previous name and follows intact.
- The default upper collection limit is **$50,000 native + wallet token holdings
  on the selected chain**. Priced lower bounds above it are excluded even with
  an incomplete inventory; exactly $50,000 remains admitted. Above-limit wallets
  pause expensive current/history work and receive six-hour screening retries.
  Their saved evidence remains inspectable under Value checks.
- Incomplete totals within the apparent range do **not** prove an upper limit.
  They remain collection candidates once the $1,000 lower bound is established,
  with **Upper limit unverified** shown. The collector now inspects wallet tokens
  when native holdings fall within the range; native-only holdings above the
  ceiling avoid token census requests. BNB/RH inventories remain partial/bounded.
- Optional `WATER_TRACKER_MAX_WALLET_USD=50000` defaults automatically; `0`
  disables the ceiling. `WATER_TRACKER_MIN_WALLET_USD=0` disables only the floor;
  set both to zero to disable capital screening. Existing budgets/volume apply.
- Validation: 140 Rust tests, Next production build, browser checks at
  320/390/1440px for persistence/search/names across surfaces, canonical addresses,
  long and escaped names, reset/cancel/keyboard focus, malformed storage, write
  failure, and visible ceiling exclusions. Rename dialog axe checks have zero
  violations; no page errors/overflow. Replay eligibility values are controlled
  test fixtures, not live financial claims. See
  [wallet names and upper limit](docs/wallet-names-ceiling.md).
Verify the merged commit's main CI and Vercel/Railway deployments plus live
behavior before claiming this follow-up is available.

PR #25's independent current activity allocation, token metadata/current marks,
balance reads and detailed saved transactions are merged. The live collection
still had 12 wallets and a 2,000/2,000 background request counter during this
follow-up; current work retained its own allocation. Historical qualification
remains strict. See [UI research and verification](docs/water-ui-research.md) and
[transaction research and setup](docs/wallet-transaction-detail.md). Populated UI
checks during the earlier outage used received records with their saved timestamps.

If a new session is picking up work:
1. Read this file.
2. Inspect `main` before changing anything.
3. Keep changes on a branch until the requested pass is actually complete.
4. Run Rust tests + Next production build + live/provider checks where relevant.
5. Only then open/merge a PR.
6. Avoid broad refactors around working paths unless they are necessary for the requested feature.

The product has already suffered regressions from “fixing the symptom” instead of the architecture. Preserve working behavior carefully.

---

# 1. Product concept

Water is an evidence-first multi-chain trading research terminal built around:

> **What does the other side of this trade see?**

The Musashi framing behind the product is “become the opponent” / “mind like water.”

The intended questions are:
- If I were the deployer, where could I extract liquidity?
- If I were an early holder sitting on a large multiple, where would I sell?
- If I were a whale, how much can I exit without crushing the pool?
- If I buy here, who is left to buy after me?
- What is the strongest evidence that my long thesis is wrong?
- If I save a thesis, has the world I entered materially changed?

Water is **not** supposed to be:
- a crypto-casino UI
- an AI “smart score” generator
- a fake probability engine
- a buy/sell bot
- a generic Dexscreener clone

Every conclusion should be traceable to visible evidence.

---

# 2. Design language

Phone-first. Dark, subdued, deliberate.

Visual direction:
- near-black / charcoal
- graphite
- warm off-white
- muted moss
- stone / bronze
- restrained clay
- no neon crypto palette
- no AI gradients
- no SaaS dashboard/card soup
- no unnecessary pills
- one idea per section
- plain-language explanation first, detail/evidence second

The user explicitly wants “smooth-brain readable” UI: dense backend logic, simple frontend interpretation.

UI skills historically referenced:
- Better UI / Jakub Krehel
- Emil Kowalski design engineering skill

Important UX rule:
- The main scan remains **one action**.
- Do not expose internal multi-stage scan steps unless debugging.
- Independent/deep evidence sections may load after the main result so they do not block the whole scan.

---

# 3. Current architecture

```text
web/  Next.js + TypeScript, phone-first PWA
  |
  v
core/ Rust + Axum + Tokio
  |
  +-- GeckoTerminal public API
  |      price / liquidity / market cap inputs / volume / pool flow
  |      token social metadata
  |
  +-- Solana JSON-RPC
  |      mint state / supply / holder authorities / history
  |
  +-- Robinhood Chain JSON-RPC
  |      contract verification / ERC-20 execution history / totalSupply
  |
  +-- Blockscout Pro
         Robinhood indexed current holders / contract creator metadata
```

No LLM is required in the runtime.
No wallet private key is accepted or stored.
No paid market-data key is required.

---

# 4. Environment

Backend:

```env
PORT=8080
GECKOTERMINAL_API_HOST=https://api.geckoterminal.com/api/v2

SOLANA_RPC_URL=https://api.mainnet-beta.solana.com
SOLANA_FALLBACK_RPC_URL=https://solana-rpc.publicnode.com

ROBINHOOD_RPC_URL=https://rpc.mainnet.chain.robinhood.com

BLOCKSCOUT_API_URL=https://api.blockscout.com/4663/api/v2
BLOCKSCOUT_API_KEY=<free Blockscout Pro key>
```

Frontend:

```env
NEXT_PUBLIC_WATER_API_URL=https://water-production-822e.up.railway.app
```

Do not commit real API keys.

---

# 5. Public API

Current endpoints:

- `GET /health`
- `POST /v1/scan`
- `POST /v1/early-holders`
- `POST /v1/origin`
- `POST /v1/token-info`
- `POST /v1/wallet-position`
- `POST /v1/holder-cohort`

Main scan request:

Solana:
```json
{"chain":"solana","address":"<token mint>"}
```

Robinhood:
```json
{"chain":"robinhood","address":"0x..."}
```

---

# 6. Main scan / Exit Pressure

Exit Pressure is a diagnostic `/100`.

**Higher = more observed exit pressure / structural fragility.**

It is NOT:
- rug probability
- scam probability
- price-down probability
- a calibrated forecast

Current weighted components:
1. wallet-holder concentration — weight 0.45
2. recent sell share — weight 0.30
3. liquidity coverage — weight 0.25

At least two components are required.

Interpretation is intentionally not hard-coded into “safe/dangerous” labels until backtesting supports thresholds.

Liquidity coverage runs in the opposite intuitive direction:
- lower liquidity coverage => more pressure

---

# 7. GeckoTerminal integration

Market scan was previously unstable because Water used two Gecko calls per scan and public rate limits were easy to hit.

Current design:
- one token request with `include=top_pools`
- parses:
  - price
  - total reserve / liquidity
  - FDV / market cap where available
  - h24 volume
  - h1 buys/sells
  - h1 unique buyers/sellers when supplied
  - h24 buyer/seller counts when supplied
- 45-second in-process market cache
- bounded retry/backoff for transient errors

Token socials are fetched separately through `/tokens/{address}/info`:
- website
- X
- Telegram
- Discord
- Farcaster
- Zora
- image metadata

Only HTTP(S) links are rendered.
Metadata failure must never block the main scan.

---

# 8. Holder concentration — important semantics

The intended metric is:

> **top 10 verified wallet-like holders / total token supply**

LP/vault/protocol-controlled balances should NOT enter the numerator.

## Solana

Do not treat `getTokenLargestAccounts` as “top wallets.” It returns token accounts.

Current preferred path:
1. enumerate mint token accounts
2. decode owner authority + amount
3. aggregate token accounts by authority
4. filter program/PDA-controlled authorities
5. rank remaining wallet-like authorities
6. sum top 10 / total supply

Authority classification:
- off-curve => excluded as PDA/program-controlled
- on-curve + executable/non-SystemProgram-owned account => excluded
- normal/nonexistent/system-owned address => wallet-like

Fallback:
- `getTokenLargestAccounts`
- resolve token-account owners
- aggregate/filter conservatively
- do NOT publish a “true top 10 wallet” metric if completeness cannot be proven

## Robinhood Chain

Current top holders come from **Blockscout’s indexed holder snapshot**, not full Transfer replay during a scan.

Flow:
1. Blockscout token holders page (already balance-ranked)
2. use Blockscout `address.is_contract`
3. exclude:
   - zero address
   - dead address
   - pool/vault/protocol contracts
4. stop fetching pages once 10 eligible wallet candidates are known
5. top 10 / direct ERC-20 `totalSupply`

This solved the old hang/missing issue caused by replaying the token’s entire Transfer history through the public RH RPC.

Important limitation on BOTH chains:
- if a protocol deliberately holds tokens in an ordinary EOA/on-curve wallet, chain state alone cannot prove it is protocol custody.
- Water must not pretend it can solve identity attribution without labels/extra graph evidence.

Denominator is still total supply, not “wallet-held float.”

---

# 9. Market-cap semantics

When Gecko does not supply market cap, Water can reconstruct:

```text
price × onchain total supply
```

This is supply-implied valuation and may differ from true circulating market cap if tokens are locked/noncirculating.

The API now exposes `token.market_cap_basis` (`provider`, `supply_implied`,
`unavailable`). The UI labels the derived value “Supply value”. Provider zero,
negative, and non-finite market caps are treated as unavailable; a finite positive
price × onchain supply can still reconstruct the valuation. Pool-level market
cap/FDV is not borrowed because it can describe the other asset in the pair.

---

# 10. Wallet cost basis / event-sourced ledger

Water does not trust a third-party PnL label.

Core rules:
- actual transaction asset deltas determine execution economics
- historical candles only convert quote asset into USD
- FIFO lots preserve remaining basis
- transfers do NOT automatically realize PnL
- ordinary wallet-to-wallet transfer does NOT inherit sender basis
- carried basis only with explicit same-owner/link evidence
- unknown inbound cost stays unknown
- unknown proceeds stay unknown
- fees are tracked separately
- ending reconstructed balance is reconciled against observed chain balance

Basis states:
- `verified`
- `partial_history`
- `incomplete`

Do not replace unknown entry with zero or candle price.

---

# 11. Large-holder / early-holder map

Endpoint:
`POST /v1/early-holders`

UI currently shows up to 3 current large wallet-controlled holders.

Important correction:
**current holder identity/balance and historical wallet reconstruction are separate layers.**

A wallet should remain visible even if its deep history fails.

Fields:
- rank
- wallet
- current quantity
- earliest observed acquisition
- peak quantity if movement history reconciles
- retained from peak
- distributed fraction
- basis coverage
- average entry USD if supported
- current price / entry multiple if supported
- movement-history status
- basis status

Meaning of “entry cost unknown”:
- the wallet is real
- current balance may be verified
- movement history may even reconcile
- but Water cannot prove the USD economic cost of the remaining tokens
- transfer-ins are a common cause

Do not show fake 100% retained / 0% distributed when movement history is incomplete.

Deep holder reconstruction concurrency is bounded (2 wallets at a time) to avoid bursting Gecko historical pricing.

---

# 12. Who Buys After Me?

Buyer-side evidence lives in the main scan response.

Current visible evidence:
- unique buyers in the last hour (when Gecko supplies it)
- unique sellers
- buy transaction share
- buyer-arrival pace:
  - last-hour unique buyers / 24h hourly average
- 24h volume / liquidity turnover

No separate buyer score has been invented.

The current product idea is to answer:
> “Is demand still arriving?”

without pretending that demand automatically means price-up.

---

# 13. Countercase

The UI has a deterministic continuation-vs-fragility section.

It uses the same already-visible evidence and neutral midpoints from Water diagnostics.

No hidden score.
No LLM narrative.
No buy/sell conclusion.

The purpose is to force the user to see both sides of the current structure.

---

# 14. WATER baseline / watch behavior

The user can save a token’s current structure as a local baseline.

Current baseline fields:
- top-wallet concentration
- buy share
- liquidity
- market cap

Later reads compare saved baseline -> current structure.

Large changes can surface as understated on-read change flags.

This is currently:
- browser-local
- same-device only
- not server-side
- not push/background monitoring

Do not describe current watch behavior as full alerts.

---

# 15. Memory / calibration

Successful scans are stored locally in browser storage.

Current limit:
- 180 observations

Water shows:
- changes since previous read
- top-wallet concentration path
- local ~1h / ~6h / ~24h price outcomes when a later scan of the same token lands in the relevant time window

Calibration is conditioned on the same **25-point Exit Pressure band** as the current setup.

Example:
```text
Pressure 25–49
~1h median outcome ...
~6h median outcome ...
~24h ...
```

This is NOT a server-side backtest and NOT a forecast.

---

# 16. Origin / control

Endpoint:
`POST /v1/origin`

## Solana

Current evidence:
- standard SPL mint authority
- freeze authority
- mint-authority current token share when readable
- launchpad recognition

Important:
- Token-2022 extension authorities are not yet comprehensively modeled
- Water must not label unrelated wallets as insiders without evidence

### Solana launchpads

Current intentional product decision:
**only Pump.fun and StonkFun are treated as relevant Solana launchpads right now.**

Do not bring back generic:
- “Raydium LaunchLab family”
- Boop
- Moonshot
- Meteora DBC

unless the user explicitly changes direction.

Pump.fun:
- requires an actually invoked Pump launch program
- core program:
  `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P`
- Mayhem:
  `MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e`

StonkFun exact fingerprints:
- standard config:
  `4E876qZTE9FJMrBzgVtBrSrzz2TLivB5Y5QXPjB4gZL7`
- reward/transfer-fee config:
  `6BwHHDg3u1854jC8PDLXvR4spTcLNaoBxLJNGC4nTESt`
- legacy launcher:
  `5CEbueQnq1Ym2uSSx2xXds3jQAqT1BDnkA59RZobSPAG`
- legacy match additionally requires Raydium CLMM:
  `CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK`

Generic LaunchLab activity alone must NOT be called StonkFun.

Current launch-history scan is bounded:
- up to 2 pages
- 1000 signatures/page
- checks oldest candidate signatures
- only actual invoked program IDs count as program evidence

### Solana launchpad logos

Local vendored assets:
- `web/public/launchpads/pumpfun.svg`
- `web/public/launchpads/stonkfun.svg`

Attribution:
- `web/public/launchpads/README.md`

The Origin UI uses local assets, not image hotlinks.

---

# 17. Robinhood Chain origin / launchpads

Robinhood origin path:
1. Blockscout contract address metadata
2. creator address
3. creation transaction
4. creator metadata/labels
5. creator token share
6. creator EOA/contract classification via RH JSON-RPC

Robinhood launchpad recognition first checks exact factory membership for Pons
v2, current/legacy Pons v1, and NOXA Fun. This path uses public RPC and does not
require Blockscout labels or an API key. See `docs/robinhood-launchpads.md` for
addresses, ABI layouts, sources, and coverage limits. Other launchpads retain
the explicit Blockscout creator/factory label fallback; arbitrary substrings no
longer count. Factory verification runs independently of optional origin indexing.

Recognized names in the current registry include:
- Pons
- hood.fun
- Long.xyz
- NOXA Fun
- Coinbarrel
- Robinpad
- StonkBrokers
- token.select
- hookr.fun
- v4.fun
- RaiseHood
- PerpsHood
- PairYard
- Pairex
- Unihood
- ArrowPad
- Ponzu
- MerryForge
- par.family
- Pyre
- Froth
- Peeps
- Pump.fun

### IMPORTANT NEXT GAP: RH launchpad logos

**Robinhood launchpad recognition exists, but per-launchpad RH logos have NOT been added yet.**

The web logo map currently has local marks only for:
- Pump.fun
- StonkFun

Other recognized RH launchpads currently fall back to a letter/avatar-style placeholder.

If the user asks to finish RH launchpad branding:
- source appropriate official/first-party marks where possible
- vendor them locally
- add attribution/licensing notes
- map their stable launchpad slugs in the UI
- do not hotlink runtime images

This is one of the most immediate unfinished UI tasks.

---

# 18. Token social links

Shown beside asset identity, not as another giant card.

Provider-backed only:
- Website
- X
- Telegram
- Discord
- Farcaster
- Zora

No fuzzy social-account matching.
No searching by token name and guessing.

---

# 19. Reliability fixes that must not be undone

## Gecko
- one request per market scan with `include=top_pools`
- 45s market cache
- retry/backoff
- socials separately loaded

## Solana
- official RPC + fallback publicnode
- holder concentration by wallet authority, not raw token account
- conservative fallback if completeness cannot be proven

## Robinhood
- current holders from Blockscout indexed snapshot
- DO NOT revert to synchronous full Transfer replay in `/v1/scan`
- holder pages stop as soon as 10 wallet candidates are found
- Blockscout contract metadata filters pool/vault contracts
- scan must not hang forever

## Frontend
- main scan has a hard stop rather than infinite “Reading”
- expensive secondary panels should not prevent base token information from rendering

---

# 20. Current UI order

Approximate scan flow:

```text
TOKEN
price · liquidity · market cap
social links

EXIT PRESSURE
structural exit constraints

BECOME THE OPPONENT
what the other side sees

LARGE HOLDER MAP
where major wallets stand

WHO BUYS AFTER ME?
is fresh demand arriving?

COUNTERCASE
continuation evidence vs fragility evidence

ORIGIN & CONTROL
mint/creator + launchpad recognition

WATER
saved baseline vs current structure

MEMORY
previous reads + concentration path + local calibration

EVIDENCE
chain/provider source ledger
```

Keep it legible on mobile.

---

# 21. What is still missing / next roadmap

These are the main unfinished pieces.

## A. Robinhood launchpad logos
Immediate visual gap.

Recognition exists.
Brand assets do not.

Add local marks + stable slug mapping for the RH launchpads that are actually useful/relevant.

## B. Robinhood execution tracing
High-value technical gap.

Current RH wallet history reconstructs ERC-20 transfers/receipts and standard top-level native ETH value + gas.

Problem:
- standard EVM RPC does not expose internal native ETH transfers
- swaps using internal ETH/WETH routing can leave quote-side economics unknown
- this contributes to `entry cost unknown`

Future approach:
- research current Robinhood/provider support first
- capability detect:
  - `debug_traceTransaction`
  - `trace_transaction`
  - provider-specific traces
- keep graceful fallback when trace methods are unavailable
- do not require a paid provider unless the user chooses one

## C. Server-side historical snapshot store + real backtester
Current history is local browser storage only.

The serious version needs:
- persistent scan snapshots
- token/time indexing
- feature snapshots
- +5m / +15m / +1h / +6h / +24h outcomes
- cohort queries such as:
  > when Exit Pressure was 50–75 and buyer pace < 1x, what happened afterward?

Only after this should thresholds be treated as statistically meaningful.

Possible infra should remain cheap/free-first.

## D. Real background alerts / watchlist
Current watch behavior is only on-read local comparison.

Future:
- saved token watchlist
- scheduled backend checks
- notify only on meaningful structural change:
  - holder concentration change
  - early-holder movement
  - liquidity shift
  - buyer-arrival collapse/expansion
  - thesis assumptions invalidated

Avoid generic price-alert spam.

## E. Deeper deployer / insider graph
Current Origin is intentionally conservative.

Future graph could connect:
- deployer / creator
- funder wallets
- pre-market recipients
- shared funding source
- launchpad factory
- transfer clusters

Do not infer “insider” from proximity alone.
Every edge should have an evidence type.

## F. Solana archival-history improvement
Current public Solana RPC reconstruction is useful but cannot guarantee complete historical wallet indexing.

A future optional archival/indexer adapter could improve:
- closed ATA discovery
- early acquisition timing
- cost-basis coverage

Keep current zero-key fallback.

## G. Token-2022 control coverage
Current Origin explicitly covers standard SPL mint/freeze authority.
Token-2022 extension authorities are not fully surfaced.

## H. Circulating-vs-total valuation labeling
Current fallback market cap is price × total supply.
Potential future UI:
- “Supply-implied valuation”
- explicit valuation basis field

## I. Cross-device WATER memory
Current baseline/history lives in browser localStorage.
Cross-device persistence would require backend storage/auth.

## J. Trading execution
Intentionally NOT the next priority.

Water should first prove that its observations/signals contain useful information through historical backtesting.

Do not rush into execution simply because the terminal looks complete.

---

# 22. Working style / project rules

The user prefers:
- implementation over generic discussion
- compact, direct communication
- mobile-first
- free/zero-key infrastructure where possible
- evidence over labels
- no fake certainty
- no “vibecoded slop”

Important behavioral rule from the user:
> **Do not open a PR before the requested feature pass is finished.**

Preferred workflow:
1. branch from latest `main`
2. finish implementation
3. run full checks
4. fix failures
5. remove temporary verification workflows
6. open PR
7. wait for normal CI/live smoke
8. merge only when green
9. verify Railway/Vercel deployment status

Do not claim something is live until deployment status is actually green.

---

# 23. Recent milestone commits

Useful history:

- `34b7825` — Pump.fun + StonkFun exact recognition + logos
- `f8a3845` — deeper holder evidence, launchpad recognition, socials, countercase, concentration trend
- `b44bebc` — position-intelligence layers: early holders, demand, origin, WATER baseline, memory/calibration
- `4a6e3ff` — stop RH holder pagination once enough wallets are known
- `9d2a7c6` — move RH current holders to Blockscout indexed snapshots
- `3e121a9` — timebox RH scans + market strip
- `c3c0991` — single-request Gecko market scan + cache
- `96838c0` — wallet-only holder concentration on both chains

---

# 24. If picking the project up right now

Recommended next order:

1. **Finish Robinhood launchpad logos/branding**
2. **Research + implement optional RH execution tracing**
3. **Design persistent snapshot schema for real backtesting**
4. **Add backend watchlist + structural alerts**
5. **Expand deployer/insider graph conservatively**
6. **Evaluate archival Solana adapter**
7. Only then consider execution/trading actions

Before touching any of those, verify the current production SOL + RH scan still works end-to-end.


## 2026-10-01 branding follow-up

Branch: `feat/robinhood-launchpad-branding`.

Added 14 first-party Robinhood launchpad marks: Pons, hood.fun, NOXA Fun,
StonkBrokers, hookr.fun, v4.fun, RaiseHood, PerpsHood, PairYard, Ponzu,
MerryForge, Pyre, Froth, and Peeps. Stable slug mapping and source URLs live in
`web/lib/launchpad-marks.json`; attribution and the eight unresolved brands are
in `web/public/launchpads/README.md`. Existing Pump.fun and StonkFun assets remain.
Unverified/ambiguous brands and failed image loads retain the initial fallback.

Validation: 48 Rust tests and Next production build passed locally. Production
health and RH WETH scan returned HTTP 200 before the changes; the SOL WSOL scan
timed out, so full production SOL health remains unverified. No backend behavior
was changed in this branding pass.

Next technical work remains optional RH execution tracing. The branding gap is
reduced, not eliminated; do not claim every recognized RH launchpad has a logo.

## Scan reliability follow-up (2026-10-01)

- `/v1/scan`: market lookup has a 12s deadline. SOL account/supply/holder reads
  run concurrently; each primary/fallback basic RPC has a 4s budget, and deep
  holder enumeration has a 9s budget. RH basic RPC calls have an 8s budget and
  retain the existing 9s indexed-holder budget. Available evidence survives a
  slow independent source. The frontend's 20s stop remains unchanged.
- Solana null `getAccountInfo.value` no longer counts as successful verification.
- Valuation: provider zeroes no longer suppress the supply × price fallback.
  Non-finite products remain unavailable, never zero. Zero transaction volume
  remains legitimate and is not filtered out.
- Origin: exact factory membership precedes strict label matching; failed
  optional explorer/creator reads no longer discard verified factory evidence.
  Factory records cannot be inferred from token names or vanity suffixes.
- Regression suite: 54 tests passed, including stalled-provider integration
  tests (available market/supply preserved; all-stalled scan returns within 12s).
- Before-PR candidate live checks: documented PONS reference recognized as
  Pons legacy v1, Pons v1/v2 and NOXA samples all recognized without a
  Blockscout key; WETH correctly unrecognized; live SOL WSOL scan returned
  in 9.0s with market data; live RH PONS scan in 0.32s. Sample addresses and
  the successful live run are recorded in `docs/robinhood-launchpads.md`.
  Temporary push-only verification workflow removed before opening the PR.
- Existing WSOL native-mint supply may be zero by SPL semantics; that does not
  imply zero valuation and is not used to invent holder concentration.


## Readability / hero pass (2026-10-01)

- Scan reliability PR #17 merged as `1d26cd5`; Railway and Vercel green.
  Production Pons legacy/v1/v2 and NOXA origin checks passed; WETH stayed
  unrecognized. Production SOL/RH scans return current market data.
- UI: brighter surfaces/text, minimum 12px labels, 16px address input,
  visible label and associated errors, split hero with scanner, compact
  results heading, explicit loading status, and explanatory empty state.
- Next production build passed. Faint-text contrast is 5.13:1–7.08:1
  across the four surfaces. Desktop and 390px/320px framed layouts reviewed
  in the authenticated Vercel preview. Narrow-body minimum reduced to 280px
  to accommodate non-overlay scrollbars at a 320px viewport.
- Preview has no backend environment configured and reports offline;
  production has the configured backend. Verify live scan interactions
  after deployment. Temporary mobile layout-check page removed before PR.
- RH coverage is not complete: direct factory verification currently covers
  Pons legacy/v1/v2 and NOXA. Other brands retain explorer-label matching.
  Next priority is verified Long.xyz evidence and other relevant factories,
  before optional RH tracing, persistent snapshots, watchlists and alerts.

## Scan recovery / smoothness pass (2026-10-03)

- Audited live desktop/mobile entry, validation, scans, origin and local baseline
  flows. Production PONS and WSOL main scans returned HTTP 200 in about 2.4s
  and 9.3s, respectively, with no browser JavaScript errors. Both large-holder
  requests hit the old 24s outer deadline; optional Blockscout creator evidence
  was unavailable while verified Pons factory evidence remained visible.
- Changing chain or editing an address now cancels the pending scan. Late
  responses cannot replace a newer result or end its loading state. Clicking
  the already selected chain preserves the current scan/result.
- Health probes stop after 5s and refresh on a scan. A failed probe reports
  unavailable and leaves scanning usable. The existing 20s scan stop remains.
- Failed browser baseline writes now show an error; a successful retry clears
  it. Validation errors clear when the address is edited.
- Robinhood factory evidence wraps long addresses at narrow widths.
- Holder-map current evidence has a 12s budget. Wallet histories share a 22s
  deadline, preserving verified candidate identities/balances and completed
  analyses before the endpoint's existing 24s hard stop. Unproven economics
  remain unknown. If candidate lookup itself fails, no holders are invented.
- Candidate validation: 58 Rust tests and the Next production build passed
  (four new stalled-source/history regressions); 12 browser recovery checks
  passed against the production build. Layouts at 320/390/768/1440px,
  keyboard focus, reduced motion and baseline persistence checked. Automated
  accessibility scan found no violations in the tested result screen; this is
  not a full accessibility certification or physical-device test.
- Next feature priority remains verified Long.xyz/other Robinhood factories,
  followed by optional execution tracing and persistent snapshots.

## Long.xyz verified launch evidence (2026-10-03)

- Recognizes the Long.xyz-authored LongLauncher at
  `0x22e99278308b393ea1260859b181ad7e78f5eeed` through its exact `LaunchCreated`
  event. Sourcify has exact creation/runtime matches and the source identifies
  long.xyz as author. Contract provenance, precise ABI and real reference
  transaction are documented in `docs/robinhood-launchpads.md`.
- Proves launcher use; which frontend submitted the transaction remains unknown.
  Generic Doppler / Uniswap infrastructure, token names and suffixes do not match.
  Newer launchers remain unsupported. No unverified logo was added.
- Chain 4663 required. Complete deployment-to-head log search uses non-overlapping
  10-million-block ranges, at most 16 requests, and a 4.5s Long-specific budget.
  Missing/partial/malformed/conflicting history remains unknown. Existing Pons /
  NOXA factory checks run independently and survive stalled Long history.
- Launch creator is read from the event; indexed contract-creator semantics stay
  intact. Evidence includes the originating transaction hash.
- Validation: 62 Rust tests and Next production build passed. Candidate origin
  endpoint verified two real Long launches, all four existing Pons/NOXA samples,
  and three negatives (WETH, Airlock, a Clanker-launched AI token) using live
  TLS-verified public RPC requests without an explorer key. Reference transaction
  receipt independently confirmed successful and contained the recorded event.
  One fast-burst Pons check returned unknown; a spaced validation run passed all
  nine controls. Public provider failures still produce unknown evidence.
- Next technical priority: optional Robinhood execution tracing, then persistent
  snapshots / backtesting. Other factory additions need the same first-party
  evidence and real positive/negative validation.

## GeckoTerminal rate-limit recovery (2026-10-03)

- Reproduced Todd `7uNUAogctSAby1pZUAt2QmBpe2bradWeep78adZmpump`: production
  GeckoTerminal returned HTTP 429 while direct chain evidence remained available.
  The token had real market data; this was a provider failure.
- Shared 24/min Gecko start budget reserves six requests for main scans;
  optional metadata/history stop at 18. HTTP 429 triggers a shared cooldown
  instead of four repeated requests. Transport/5xx retries are bounded.
- Main reads coalesce concurrent requests and cache successes for 45s. Solana
  provider keys retain base58 case. A failed Gecko read can use the free
  Dexscreener exact-chain/base-token deepest-pool feed within the existing scan
  deadline. Both sources failing still means unknown.
- Source ledger now exposes failure causes and labels fallback receipt/scope.
  Unique wallet counts remain unknown on fallback; historical wallet cost basis
  never substitutes current Dexscreener prices for historical evidence.
- `token.market_data_basis` records provider/pool scope. Baseline market changes,
  memory deltas and calibration are compared only within the same basis, while
  holder concentration remains comparable independently. Legacy saved reads are
  Gecko-based. Scored calibration starting-read counts now compare band values.
- Validation: 67 Rust tests, Next build, healthy Todd live-provider scan, reproduced
  429 with live fallback/RPC data, and production-build browser checks at
  320/390/768/1440px passed. Baseline/memory provider-change guards and baseline
  reset checked; no browser errors or overflow. Details: docs/market-data.md.

## Wallet tracker research (2026-10-03; not deployed)

- Current completed production baseline is main
  `fbefa2a2c36d42c55f1b8fbf92dac41334b950e1` (PR #22, Gecko recovery).
  This research pass changes documentation only.
- User wants automatic discovery/ranking and continuous observation of strongly
  profitable wallets with 30–60-day records, including Pump.fun, Fomo.family and
  relevant directly discovered onchain wallets on Solana and RH 4663. User
  rejected a $49/month data subscription; use free public/free keyed access.
- Read `docs/wallet-tracker-research.md` for the pinned community-client audit,
  first-party contracts, actual free-provider probes, accounting requirements
  and forward paper evaluation. `docs/wallet-tracker.md` is the implementation
  brief; machine evidence is in `docs/research/wallet-tracker/`.
- Do not adopt FOMOTrade as the runtime or use its pagination-success flag for
  qualification. Exact-source replay found eight false-success cases. Its 200
  client/poller tests passed with external/native HTTP disabled; the initial
  full suite had five failures caused by an omitted Pump transport stub, and
  those five passed when isolated. No real login tokens, trades or messages used.
- Fomo's public frontend independently confirms `lastSwapIdV2`, `lastTransferId`
  and hasNextPage continuation. Authenticated 60-day history remains untested.
  Its current terms section 16 prohibit automated collection; a normal login
  alone does not establish a permitted Water feed. No access request was sent.
- Helius documents a free 1M-credit plan and efficient historical SOL RPC; no
  local key available for acceptance testing. Existing production Blockscout
  free key needs address/internal-transfer paging validation. Alchemy free
  archive/Transfers access excludes Debug/Trace and does not establish RH
  internal transfers.
- RH dRPC public access returned September 1 and August 4 call traces. The old
  trace included native ETH calls and matched official transaction/receipt
  evidence. Timeouts and an earlier method failure also occurred; this proves
  sampled free archive capability, not complete-wallet coverage or reliable
  monitoring. DELEGATECALL inherited value must not be counted as another ETH
  transfer.
- Next engineering gates: free wallet-wide history and opening-basis proof,
  real native-swap reconciliation, durable storage within the existing budget,
  strict coverage/accounting adapters, automatic discovery/UI, restart/gap
  recovery and a forward paper evaluation. Never qualify missing/stale history
  or assume provider PnL proves skill. No tracker endpoint, ranking, background
  monitoring or alerts are deployed.

## Wallet tracker implementation (2026-10-04; not deployed)

- Branch `feat/wallet-tracker` implements `/wallets` plus four Rust endpoints,
  bounded automatic Pump/optional independent Fomo/onchain discovery, a single
  background collector, SQLite evidence/cursors and persisted daily quotas.
  Read `docs/wallet-tracker-implementation.md` for supported semantics, acceptance
  evidence and deployment setup. Existing token scans retain their provider path.
- Rankings use two independent FIFO months with opening inventory, network fees,
  open losses, episode/sample/profit-factor/outlier checks, canonical order and
  freshness. Standard execution-account verification excludes protocol accounts.
  Unknown prices on positions closed before the window do not poison later
  known rounds. Unknown opening basis/current economics continue to block ranks.
- SOL recognizes scoped Pump bonding-curve/Jupiter instructions, cancels owned
  token-account rent/wrapping and verifies the actual fee payer. RH uses finalized
  receipts, historical state, native call traces and a Sourcify-verified V3 runtime
  template. Inherited delegate values, reverted calls, fake topics and unresolved
  sponsored fees cannot qualify. No provider leaderboard PnL becomes Water PnL.
- The responsive UI includes research/qualified/following lists, received activity,
  detail/gates/coverage, local follows, independent qualified token overlap and
  scanner links. Public nominations default off; no trades or private Fomo
  polling, alerts, ownership merging or creator graph claims were added.
- Validation: 89 Rust tests, Next production build and 15 browser checks passed.
  Local stop/start preserved 1,476 records, two continuations and daily quotas.
  Final SOL/RH live scans received market and direct-chain data with collection
  active (1.08s/1.33s). The archived Aug 4 RH swap reconciled native input, gas
  refund/payment and verified runtime. Evidence summaries are committed under
  `docs/research/wallet-tracker/`; no complete real-wallet 60-day rank is certified.
- Production activation still requires a durable Railway volume/path and one
  collector instance, a free Helius key, validation of the existing RH index key
  across every address-history route and archive traces, and optional permitted
  Fomo discovery credentials. No such secrets are available in this workspace;
  configure them on the server, never paste trading/login keys into the UI.
  Without a database path the collector is paused and no rank is invented.
- Follow-up work: keyed real-wallet acceptance, measure free credits/backfill/disk
  growth, then forward cohort evaluation. PumpSwap/custom execution, smart-account
  fees, funding/creator graphs and push alerts require later supported adapters.
  Production remains main `fbefa2a` until this branch is merged and deployments
  are verified; do not claim the tracker is live.


## Helius budget and BNB pass (2026-10-04; PR #23, not deployed)

- Helius Free is 1M credits per project per credit cycle, not 1M full-history
  calls. Water requests at most 100 full transactions/page (ten-credit minimum).
  Default 2,000 HTTP attempts/day would reserve at most 620,000 credits over 31
  UTC days even if every attempt were a ten-credit Helius call. No paid plan,
  purchase or autoscaling is configured.
- Legacy `HELIUS_API_KEY` works; optional comma-separated `HELIUS_API_KEYS` trims
  and deduplicates credentials (up to eight). HTTP 401/403 can fail over; 429
  pauses an hour rather than switching around quota. Every key shares Water's
  atomic SQLite budget, default `WATER_TRACKER_HELIUS_CREDITS_31D=800000`.
  Counters include failed attempts, survive restarts/key changes and cover the
  current plus preceding 31 UTC date buckets conservatively. UI values are local
  reserved estimates, not actual provider remaining credits.
- BNB API/storage identity is `bnb`, mainnet chain 56, market slug `bsc`, native
  BNB. Scans receive market, contract and ERC-20 supply evidence; complete holders,
  creator and deep per-token wallet/cohort reconstruction remain unknown. `owner()`
  is an owner observation, not creator/control proof. Public node fallbacks must
  independently identify chain 56. Scanner controls/prefills, wallet filters,
  follows, explorer links and qualified-overlap identity include BNB.
- Bounded automatic BNB discovery requires successful finalized Pancake V2
  receipts, Sourcify runtime, historical tokens and documented factory/getPair
  membership. Discovery gives each chain a turn before filling a new cohort;
  existing wallets are retained. Free-provider archive/rate failures may yield
  no new BNB nomination, which is not complete discovery. Four.meme/Pancake V3
  and other venues require additional adapters.
- BNB observation scans received token identities in address-scoped log windows,
  preserving/merging empty block intervals. It cannot prove wallet-wide native,
  failed, internal-only or unobserved-token history, so BNB 30/60-day qualification
  is unavailable. Actual receipts remain visible through archive failures with
  unresolved execution/unknown amounts and no fabricated native proceeds.
  Missing precision does not create a normalized quantity; partial records retry.
  BNB gas fees use standard receipt gas economics, separately from RH/ETH.
- Validation: 100 Rust tests and Next production build passed. The existing 15
  browser checks and 11 BNB/credit browser checks pass; latest scan timings are recorded
  in `docs/research/wallet-tracker/bnb-and-credit-checks.json`. Active-collector
  scans received SOL/RH/BNB market and chain evidence. A received BNB wallet was
  manually nominated for local acceptance; the collector then found additional
  real transfer references and retained partial receipt activity. This does not
  certify automatic discovery coverage or a full real-wallet profitable record.
- Read `docs/wallet-tracker-bnb.md` for sources, configuration and free BNB history
  constraints. Etherscan's free history matrix excludes BNB; the NodeReal free
  history/native category contract still needs keyed reconciliation. No paid
  provider was substituted. Production volume, Helius/RH credentials and keyed
  real-wallet acceptance from the previous handoff remain outstanding.

## Tracker RPC recovery and Pump scope (2026-10-04; review branch)

- Branch `fix/tracker-rpc-pump`, based on main `cd0fe216`. PR #23 is merged and
  deployed; this follow-up is not deployed until separately merged.
- Production now reports storage configured and collection enabled, two Helius
  credentials, an RH index key and six received SOL/RH candidates. The earlier
  deployment setup notes are historical. A path flag alone still does not prove
  volume durability; Fomo discovery has no configured permitted access.
- Reproduced RH `-32000` as unavailable historical state on the official RPC.
  Free dRPC returned code, precision and balance at the same finalized height.
  Account/balance checks now use archive fallback with a pinned finalized block;
  fallback and trace chains are checked. Unknown reads never become `latest`.
- Configured Helius now serves SOL state and bounded discovery reads, sharing
  indexed history's persisted budget and cooldown. Previously these checks
  bypassed Helius and received HTTP 403 from the public fallback.
- BNB was already in discovery, history, activity, filters and follows. Its
  public known-token history cannot prove wallet-wide completeness, so it stays
  under research. No BNB production nomination was observed in this sample.
- Pump's mobile/site interface is multichain, including RH and BNB references.
  Its currently received public monthly board lacks RH/BNB execution-wallet
  mappings. Removed `0x` => RH guessing and ambiguous multichain profile seeds.
  No authenticated Pump social history or complete three-chain social feed was
  implemented. Discovery notes expose this gap; see the linked research for the
  permitted-feed requirements and the distinction from Go.fun bounty payouts.
- Validation: 108 Rust tests, production web build, four viewport browser checks
  and actual Rust provider state reads on the screenshot's RH wallet passed.
  Pending history/price/trace/basis gaps remain separate from repaired state reads.
- Details and captured evidence:
  [RPC and Pump coverage](docs/wallet-tracker-rpc-pump.md).

## Collection, stale evidence and wallet deadline (2026-10-05; PR #24, not deployed)

- Live check at 05:50 UTC: all 12 candidates were stale; the tracker reported
  2,000 / 2,000 daily requests used. Newest saved collection was 02:24:32 UTC.
  The separate Helius counter was 8,980 / 800,000 estimated credits over 31 days.
- Three production detail requests took 0.499–0.569 seconds. No spontaneous
  timeout was reproduced. A controlled 13-second delay proved the production
  client aborted after 12.459 seconds; the user's latency cause remains unknown.
- Split current checks, history and discovery within the existing daily cap:
  70% / 25% / 5%. Current checks have priority and a default 40-minute target;
  history is paced and normally waits for a 25-request batch. Discovery runs
  only when the cohort has space. Each pass has a 30-second time limit.
- Pages, records and received state are committed and analyzed before subsequent
  slow work. Global and rolling credit counters survive the upgrade unchanged.
  Today's spent budget still waits for UTC midnight; do not reset it to claim
  collection has recovered.
- Account/balance state has its own freshness timestamp. A current head page
  cannot qualify old state. A tokenless BNB chain check cannot refresh history.
  Full historical/trace/price/basis gaps still withhold economic qualification.
- Default to research, show actual saved/pending counts and latest known
  execution, put activity before month checks, and make all gates expandable.
  Wallet client deadline is 30 seconds; failed refresh retains open evidence.
- Validation: 114 Rust tests and Rust/web builds passed; controlled browser
  checks at 320/390/768/1440 passed with captured production data. A 13-second
  response now succeeds; a stalled refresh aborts at 30.456 seconds without
  clearing saved details. CI for the final published head must be checked.
- No new required variable or paid provider. Optional
  `WATER_TRACKER_REFRESH_SECONDS=2400` controls the current-check target.
  [Collection details and captured evidence](docs/wallet-tracker-collection.md).
