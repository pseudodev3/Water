# Water

Water is an evidence-first, multi-chain trading research terminal built around one question:

**What does the other side of this trade see?**

V1 supports Solana and Robinhood Chain with no paid market-data key required.

## Data path

```text
web/  Next.js phone-first UI
  |
  v
core/ Rust + Axum
  |
  +-- GeckoTerminal public API   price / liquidity / volume / pool flow
  +-- Solana JSON-RPC            mint + supply + largest token accounts
  +-- Robinhood JSON-RPC         contract verification
  +-- Robinhood JSON-RPC         contracts / ERC-20 execution history
  +-- Blockscout Pro             indexed RH current holders / contract origin
  +-- engine.rs                  deterministic diagnostics
```

Water does not invent missing data and does not issue buy/sell instructions. Every pressure component is derived from visible source values.

### Current pressure inputs

- top-ten holder concentration
- one-hour sell transaction share
- DEX liquidity coverage relative to market cap / FDV

The index is a diagnostic, not a prediction or calibrated probability.

## Run locally

No GMGN key, LLM key, wallet private key, or paid market-data key is required. Robinhood current-holder/origin intelligence uses a free Blockscout Pro API key.

```bash
cp .env.example .env
cargo run -p water-core
```

Then:

```bash
cd web
npm install
npm run dev
```

## UI skills

```bash
sh scripts/install-skills.sh
```

Installs:

- `better-ui` by Jakub Krehel
- `emil-design-eng` by Emil Kowalski

## API

- `GET /health`
- `POST /v1/scan`
- `POST /v1/early-holders`
- `POST /v1/origin`
- `POST /v1/wallet-position`
- `POST /v1/holder-cohort`

Solana:

```json
{"chain":"solana","address":"<token mint>"}
```

Robinhood Chain:

```json
{"chain":"robinhood","address":"0x..."}
```

## Limits

GeckoTerminal's public API is rate-limited and cached, so Water V1 is a research terminal rather than a low-latency execution engine. Paid/indexed providers can be added later behind adapters without changing the evidence model.


## Position-intelligence layers

The main scan remains one action. Deeper layers render independently so an expensive wallet-history reconstruction cannot block basic token/market evidence.

### Early-holder map

`POST /v1/early-holders` reconstructs up to three current large wallet holders and exposes only evidence Water can support: earliest observed acquisition, current vs peak inventory, distribution, basis coverage, known average entry and current-price/entry multiple.

The sample is explicitly **current large wallets ordered by earliest observed entry**, not a claim to have discovered the first buyers ever.

### Who buys after me?

The scan response includes buyer-side evidence from the same GeckoTerminal top-pool payload already used for market structure:

- unique buyers and sellers in the last hour when supplied by GeckoTerminal
- buy transaction share
- last-hour unique-buyer pace versus the 24-hour hourly average
- 24-hour volume relative to observed liquidity

No additional score is invented from these values.

### Origin & control

`POST /v1/origin` exposes direct origin/control facts only.

- Solana: standard SPL mint authority, freeze authority and the mint authority's current token share when readable.
- Robinhood Chain: indexed contract creator/creation transaction from Blockscout plus creator token share and direct RPC classification.

Water does not infer unrelated wallets to be insiders.

### WATER baseline

The web UI can save a token's current structure as a local baseline and compare later scans against it: wallet concentration, buy share, liquidity and market cap. Large changes are surfaced as explicit on-read change flags using visible sensitivity thresholds; they are not trading signals. The baseline stays in browser storage and is not uploaded to Water.

### Memory & calibration

Successful scans are retained locally on the device (bounded to 180 observations). Water shows changes since the previous read and derives local ~1h, ~6h and ~24h outcome samples only when a later scan of the same token falls inside the defined time window.

This is personal calibration evidence, not a forecast and not a server-side backtest dataset.

## Evidence-depth behavior

### Large-holder rows survive incomplete history

Water now separates **who currently holds tokens** from **what Water can prove about that wallet's past**.

The holder candidate source establishes the current wallet and current quantity. Historical movement reconstruction is a second layer. If that history is partial or unavailable, the wallet still appears; unsupported peak, distribution, and entry fields stay unknown instead of dropping the holder or inventing zeros.

`entry cost unknown` means the wallet is real and its movement history may reconcile, but Water cannot prove the USD economic basis of the remaining tokens. A transfer-in is one common example.

### Launchpad recognition

Origin evidence can recognize launchpad families only when Water has direct support:

- Solana: a bounded mint-history transaction is inspected for known launch-program IDs. Shared programs are labeled as families rather than falsely naming a specific frontend.
- Robinhood Chain: the indexed contract creator/factory is matched against Blockscout's own contract labels/tags.

No match is reported as `Not identified`, not guessed from token names or symbols.

### Token links

Website/social links come from GeckoTerminal's token-info metadata and are fetched independently of the main scan. Only HTTP(S) links are rendered; a metadata failure does not block market or chain analysis.

### Countercase and concentration path

The countercase places the same visible evidence on a continuation side or fragility side using the neutral midpoints already exposed by Water's diagnostics. It does not add a hidden score.

Saved local reads also show a top-wallet concentration path. Local outcome calibration is conditioned on the same 25-point Exit Pressure band as the current read so unrelated pressure regimes are not mixed together.

## Security

V1 is read-only. It does not need, accept, or store trading private keys.


## Wallet cost basis

Water reconstructs wallet positions from transaction evidence rather than trusting a third-party PnL label.

### Economic rules

- Execution cost comes from the wallet's actual asset deltas in the transaction.
- Historical market prices are used only to convert the quote asset into USD.
- FIFO lots determine the basis of the remaining position and realized disposals.
- Network fees are tracked separately from execution price.
- An ordinary wallet-to-wallet transfer does **not** automatically inherit the sender's economic basis.
- Carried basis is only valid when separate evidence establishes that economic ownership did not change.
- Unknown cost stays unknown. Water reports basis coverage instead of substituting zero or a candle guess.
- Unknown sale proceeds stay unknown. Distribution still counts, but total realized PnL is withheld.
- A reconstructed ending balance is compared with the current chain/indexer balance.

### Basis states

`verified`
: Historical discovery completed, current balance reconciles, the open position has complete basis, required quote prices and fee prices are present, and the ledger has no reconstruction warnings.

`partial_history`
: Water has useful reconstructed evidence, but at least one history, pricing, fee, or discovery condition is incomplete.

`incomplete`
: Available public evidence cannot support a complete economic basis for the wallet/token pair.

### Solana historical token accounts

SPL tokens live in token accounts rather than directly on the wallet. Water starts from the wallet and current target-token accounts, then inspects historical transaction balance metadata for additional target-token accounts owned by that wallet. Newly discovered accounts are scanned recursively. This recovers many accounts that were later closed.

Vanilla RPC still does not provide the same completeness guarantee as a dedicated archival wallet index, so Water keeps this history marked as partial and relies on ending-balance reconciliation plus basis coverage rather than claiming perfect discovery.

### Robinhood Chain

Wallet cost-basis history itself does not depend on Blockscout. Historical ERC-20 activity is discovered from the token contract's `Transfer` logs over Robinhood JSON-RPC. Water automatically splits `eth_getLogs` ranges when a node rejects a wide/high-result query, then reconstructs wallet ERC-20 deltas from transaction receipts and reconciles the ending position with `balanceOf`.

Standard EVM RPC does not expose internal ETH transfers. When a swap's quote leg is native ETH delivered through an internal call, Water leaves that proceeds/cost leg unknown rather than inventing it. `ROBINHOOD_RPC_URL` stays configurable so an archive/trace-capable provider can be dropped in later without changing the ledger.

### Historical USD pricing

Quote-asset quantities come from the transaction itself. GeckoTerminal OHLCV is used to convert the quote asset to USD:

- hourly close preferred
- daily close is an explicit fallback
- missing price remains missing
- repeated quote windows are cached in the Rust service

## Position APIs

### Wallet position

`POST /v1/wallet-position`

```json
{
  "chain": "robinhood",
  "token": "0x...",
  "wallet": "0x...",
  "launch_timestamp": 1790000000
}
```

Returns the reconstructed ledger, quantitative holder behavior, history coverage, price coverage, current-balance reconciliation, and warnings.

### Holder cohort

`POST /v1/holder-cohort`

```json
{
  "chain": "solana",
  "token": "<mint>",
  "limit": 5
}
```

The first cohort implementation analyzes **current large holders**, then orders them by the earliest acquisition Water can observe. It is intentionally not described as "the first buyers": wallets that already exited are not present in a current-holder sample, and Solana history may omit previously closed token accounts.

The cohort reports numeric retention, sold fraction, transferred-out fraction, distribution, basis coverage and weighted entry where supported. Water does not manufacture "smart money" or "insider" labels from those numbers.


## Robinhood holder indexing

Robinhood market data, contract verification, and ERC-20 supply remain available through the public providers. Current top-wallet reconstruction uses Blockscout's indexed holder snapshot instead of replaying the chain's entire Transfer history during a scan.

Set `BLOCKSCOUT_API_KEY` on the backend to enable Robinhood wallet-holder concentration and holder-cohort candidates. The indexed holder addresses are still checked with Robinhood JSON-RPC so contract-controlled pools/vaults are excluded before wallet ranking.
