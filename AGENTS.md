# Water engineering rules

## Product contract
Never fabricate token, wallet, liquidity, PnL, or chain data. Missing data renders as missing. Every derived diagnostic must be reproducible from source values. AI may explain evidence later; AI is never the market-data source.

## Architecture
Keep the system boring until measured load requires otherwise.
- One Rust core service.
- One Next.js web app.
- Provider-specific code stays behind provider modules.
- Chain evidence stays behind chain modules.
- Shared analytics operate on normalized data only.
- No queues, service mesh, or extra databases without a measured need.

GMGN is the primary market-data provider for Solana and Robinhood Chain. Direct RPC evidence remains separate.

## UI
Before substantial UI work, install/read:
```bash
npx skills add https://github.com/jakubkrehel/skills --skill better-ui
npx skills add https://github.com/emilkowalski/skills --skill emil-design-eng
```

Water should feel quiet, precise, and expensive.
- Avoid generic crypto neon.
- Avoid excessive cards and pills.
- Use borders only for structure and subtle inset shadows for depth.
- Never use `transition: all`.
- Pressable controls use subtle scale feedback.
- High-frequency interactions should be instant or nearly instant.
- Respect reduced motion.
- Never trade readability for atmosphere.
