# Water — Project Handoff

_Last updated: 2026-10-01_

This file is the canonical pickup point for a new ChatGPT/Codex session.

## 0. Start here

Repository: `pseudodev3/Water`

Production:
- Frontend: `https://web-water.vercel.app`
- Backend: `https://water-production-822e.up.railway.app`

Feature baseline immediately before this handoff:
- `34b7825e9a50b2e4e76be37a4f66abf419657080`
- PR #15: **Recognize Pump.fun and StonkFun specifically**

At the time this handoff was written:
- Railway deployment: green
- Vercel deployment: green
- Open PRs: none

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

UI currently still calls this “Market cap.”
A future refinement could expose `valuation_basis` or say “supply-implied MC.”

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

Current Robinhood launchpad recognition is label-based from Blockscout creator/factory metadata.

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
