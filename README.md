# Water

Water is an evidence-first, multi-chain trading research terminal built around a simple idea: **understand the position of the people on the other side of your trade**.

V1 supports Solana and Robinhood Chain, uses GMGN as the primary market-data provider, verifies assets directly against each chain, and derives a transparent exit-pressure diagnostic from observable holder/trader data.

Water does **not** issue buy/sell instructions. It exposes positioning, pressure, provenance, and the evidence behind each derived metric.

## Architecture

```text
web/  Next.js phone-first UI
  |
  v
core/ Rust + Axum
  |
  +-- providers/gmgn.rs        shared market data (sol + robinhood)
  +-- chains/solana.rs         direct Solana account verification
  +-- chains/robinhood.rs      direct EVM contract verification
  +-- engine.rs                deterministic diagnostics
```

The first release stays intentionally small: one Rust service, one web app, no queues or pretend microservices.

## Run locally

```bash
cp .env.example .env
# set GMGN_API_KEY
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

Installs `better-ui` and `emil-design-eng`.

## API

- `GET /health`
- `POST /v1/scan`

```json
{"chain":"solana","address":"<token address>"}
```

```json
{"chain":"robinhood","address":"0x..."}
```

## Security

V1 is read-only. It does not need, accept, or store trading private keys.
