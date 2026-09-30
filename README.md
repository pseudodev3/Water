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
  +-- Robinhood JSON-RPC         ERC-20 logs / receipts / balances
  +-- engine.rs                  deterministic diagnostics
```

Water does not invent missing data and does not issue buy/sell instructions. Every pressure component is derived from visible source values.

### Current pressure inputs

- top-ten holder concentration
- one-hour sell transaction share
- DEX liquidity coverage relative to market cap / FDV

The index is a diagnostic, not a prediction or calibrated probability.

## Run locally

No GMGN key, LLM key, wallet private key, or paid API key is required for V1.

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

Water no longer requires Blockscout. Historical ERC-20 activity is discovered from the token contract's `Transfer` logs over Robinhood JSON-RPC. Water automatically splits `eth_getLogs` ranges when a node rejects a wide/high-result query, then reconstructs wallet ERC-20 deltas from transaction receipts and reconciles the ending position with `balanceOf`.

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
