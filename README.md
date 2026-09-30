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
  +-- Robinhood Blockscout       indexed token holders
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
