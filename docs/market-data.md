# Current-market recovery

Reproduced 2026-10-03 with Solana Todd
`7uNUAogctSAby1pZUAt2QmBpe2bradWeep78adZmpump`: production `/v1/scan`
returned valid mint/supply/holders but no market values. Its source ledger
reported GeckoTerminal HTTP 429. The exact token was indexed by both public
providers; direct reads from a different origin returned HTTP 200.

## Provider order and budgets

- GeckoTerminal is primary: one token request with `include=top_pools`.
- Main reads share a per-token in-flight gate and a 45s successful-read cache.
  Failed reads are shared for only 1s so a subsequent retry can recover.
- All Gecko requests, including optional metadata and historical pricing, share
  a rolling cap of 24 starts per minute per service instance. Optional work
  stops at 18, reserving six starts for scans. This protects the local allowance;
  shared provider IP limits can still be stricter.
- HTTP 429 starts a shared cooldown (numeric Retry-After, 1–300s, default 60s).
  Water returns the cause immediately and does not repeat a throttled request.
  Transport / 5xx reads have at most two 3s attempts, separated by 250ms.
- Main Gecko lookup has a 7s budget; a failed lookup may use Dexscreener's free
  `/token-pairs/v1/{chain}/{token}` endpoint within another 4s. The existing 12s
  outer market deadline and independent chain evidence remain intact.
- Both provider failures stay unknown. No stale values or zero-filled economics
  substitute for missing market data. No API key is required by the fallback.

Solana cache and matching keys preserve base58 case; EVM keys may be lowercased.

## Fallback evidence

Rows must match the requested chain and exact **base token** address. A quote
token match is insufficient because the API's USD price describes the base.
Among eligible priced rows, Water selects the highest reported USD liquidity.
Only finite nonnegative values are accepted; price/cap/FDV must be positive.

Fallback price, liquidity, volume and hourly buy/sell transaction counts refer
to that selected pool. Liquidity is not an aggregate across the token's pools.
Unique buyers/sellers and buyer-arrival pace are unknown because this endpoint
does not supply them. Historical cost basis never uses this current-price feed.

The source ledger retains the failed Gecko status and separately labels the
received Dexscreener fallback, with the pool address and scope. API token field
`market_data_basis` is `geckoterminal:aggregate`, `dexscreener:{pool}`, or null.
Local baselines and calibration preserve this basis. Market changes/outcomes
are compared only within the same basis; holder concentration can still be
compared independently. Legacy saved reads predate fallback support and are
treated as Gecko reads. A user can save the current setup to start a new basis.

## Validation

- 67 Rust tests and the Next production build passed.
- Eight concurrent same-token reads issue one primary request; a simulated 429
  produces one labelled fallback request and propagates cooldown to metadata.
- Tests cover healthy-primary preference, reserved scan capacity, cooldown
  recovery, exact chain/base-token matching, quote rejection, base58 case,
  deepest-pool choice, unsupported unique counts, missing data and later retry.
- Candidate Todd main scan used real TLS-verified Gecko/RPC data successfully.
  A reproduced HTTP 429 with live TLS-verified Dexscreener/RPC responses recovered
  Todd's current price/liquidity/cap and preserved unknown unique-wallet counts.
- Production-build browser checks at 320/390/768/1440px found no overflow or
  JavaScript errors. Legacy Gecko baseline/memory did not produce market deltas
  or calibration samples against the fallback. Saving a new baseline recovered
  comparable market deltas.

Public endpoints:
https://api.geckoterminal.com/api/v2/networks/solana/tokens/7uNUAogctSAby1pZUAt2QmBpe2bradWeep78adZmpump?include=top_pools
https://api.dexscreener.com/token-pairs/v1/solana/7uNUAogctSAby1pZUAt2QmBpe2bradWeep78adZmpump
