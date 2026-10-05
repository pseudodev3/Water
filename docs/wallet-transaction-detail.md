# Wallet transaction detail and continuous collection

Research and implementation checked 2026-10-05 against `main` at
`3fa8cdc53bfbcc4b2c5aed969004b556e8c66128`. This is a branch change; production
continues using main until the PR is merged and deployments succeed.

## What the live audit actually received

All twelve production wallet details were read, not inferred from the screenshot.
Seven wallets were Solana, two Robinhood and three BNB. The API reported the
shared daily request budget exhausted at 2,000/2,000, with only 8,980 Helius
credits reserved against an 800,000 rolling allocation. All head timestamps were
stale. Of 886 positive **reconstructed** positions, 885 lacked USD values. This
count does not prove that those tokens are currently held: incomplete histories
cannot establish ending quantities.

The detail responses contained 794 activity events; 696 lacked USD values.
The page displayed only twelve events and omitted already-received quote
amounts and valuations. The backend truncated activity at one hundred events,
while unparsed references had no activity row. Names and tickers had no persistent
registry. Historical pricing was reached through the same paused historical
collection path; it also attempted only four rotating assets per wallet pass.

Independent, public batch reads returned exact token identities on all three
chains. The initial samples returned 12/12 Solana, 11/11 Robinhood and 1/1 BNB
token rows from GeckoTerminal. Dexscreener returned 12/12, 3/11 and 1/1,
respectively. **A returned token row does not guarantee a received USD price.**
The implemented clients were then checked against thirty requested Solana assets,
eleven Robinhood assets and native BNB. Matching includes the provider network and
exact address; a foreign-chain pair or quote-token price cannot price the asset.

Five representative transactions from the existing cohort were re-fetched through
the actual provider adapters: two Solana, two Robinhood and one BNB. All five
returned evidence. The BNB transaction remained incomplete because trace/pool
proof was unavailable; its receipt and network fee remained inspectable.

## Collection that survives a background pause

Current work and background work now have independent HTTP allocations.
`WATER_TRACKER_DAILY_REQUESTS` is the historical/discovery allocation (default
2,000; history 95%, discovery 5%). `WATER_TRACKER_CURRENT_DAILY_REQUESTS` defaults
to 24,000 and bounds head checks, recent EVM resolution and current balance reads.
The existing global request total remains the total of all attempted requests.
Background spending is total minus known current spending; unidentified legacy
spending remains charged to background. No counter is reset on deployment.

The collector checks up to four different due wallets together, then runs
historical work serially on later ticks. Head and history do not concurrently
write the same wallet's coverage or continuation cursors. Current EVM work resolves
one recent pending reference per pass and retries failures with a delay. Solana's
indexed head already includes normalized transactions. Current state reads are
paced to thirty minutes: complete token-account reads for Solana and at most two
known token balances per EVM wallet, oldest balance first. The durable head page
is committed before these additional reads, so a later timeout cannot erase it.

The target refresh default is six minutes. The effective interval grows with the
cohort's measured chain composition and conservative estimated request/credit
cost. For seven SOL and five EVM wallets at the default current allocation, the
estimated HTTP floor is 630 seconds. Received current-request counts and completed
check counts further lengthen cadence when actual RPC costs are higher or the
remaining daily allocation is smaller; the worker tick and provider time add latency.
This is polling, not a promise of instantaneous updates. A configured longer
`WATER_TRACKER_REFRESH_SECONDS` continues to take precedence.

Helius accounting now reserves the method's documented cost before sending:
standard RPC costs one credit; full indexed history costs ten per hundred requested
transactions, rounded up, minimum ten; DAS metadata batches cost ten. Unknown
methods reserve a conservative hundred. Failed attempts remain conservatively
charged locally. All configured credentials share the same ledger; adding keys
does not expand a project's free allowance.

The rolling budget still counts up to 32 UTC date buckets to conservatively cover
31 days across midnight. Daily Helius shares use that divisor: 80% for current
work, 15% for paced history, 5% for discovery. History cannot spend its whole day
at startup. The provider's actual monthly/rate limit, a local current allocation,
a record ceiling, storage failure or unavailable RPC can still stop the affected
work. The status distinguishes `background_paused` from `budget_paused` and
reports Helius exhaustion separately. These are bounded free-provider operations,
not unlimited collection.

## Token identity, balances and current USD values

An additive SQLite `token_quotes` table persists address, name, ticker, decimal
precision, optional current USD mark, read time, source and missing-data detail.
Names are labels received from providers, not authenticated project identities.
GeckoTerminal batches are primary; Dexscreener exact-chain base-token pools are
fallback, selecting the deepest received pool. Native addresses are mapped to
explicit wrapped-native contracts, preserving the original asset identity.
Helius DAS supplies fungible metadata when permitted by the background allocation;
NFT interfaces and unsolicited IDs are not accepted as fungible market evidence.
No paid adapter or new key is required.

The entire cohort shares up to six thirty-token batches per minute, distributed
fairly across chains. Positive reconstructed holdings receive priority; oldest
marks are refreshed before newer marks. This avoids refreshing only the first
thirty holdings in a large wallet. Closed assets and historical activity token
labels remain in the metadata backlog. Successful prices refresh after ten
minutes; unavailable markets retry after an hour. Transport/rate failures retry
after two minutes rather than being mistaken for unindexed markets. Gecko's
existing shared 24/minute limiter, optional-work cap and cooldown still apply.
Main token scans retain their reserved allowance.

Each position exposes the reconstructed quantity separately from its valuation
quantity. A received RPC balance takes precedence and includes its asset-specific
read time and finalized block/slot reference. Partial EVM balance refreshes do not
claim full-wallet reconciliation or refresh qualification's full-state clock.
A complete Solana inventory can establish zero for a token account that has closed.
Received balances with no reconstructed position are still visible, with unknown
basis. If no balance was received, the displayed value explicitly uses the saved
transaction reconstruction and its history time.

The calculation is `valuation quantity × current USD mark`, using Decimal
arithmetic. Marks older than fifteen minutes cannot supply a current value. A
received zero quantity has zero marked value even without a price. Missing prices
stay unavailable and explain why; zero, missing and stale are different states.
The price, source and quantity timestamps are visible. Indexed marks do not prove
liquidity or executable exit proceeds. Current marks never enter historical PnL,
entry prices or qualification calculations. Average open entry stays unavailable
unless saved history and balance reconciliation support the current quantity;
individual transaction entry/exit ratios remain inspectable separately.

## Entry, exit and important transaction evidence

`POST /v1/wallets/activity` accepts `chain`, `wallet`, optional `cursor`, and a
bounded `limit` (default 25, maximum 100). It pages **all saved records**, including
pending, failed and incomplete transactions. This does not claim that the saved
records cover the complete source history. Cursors include a fingerprint of saved
record evidence. If a head update or resolved record changes it, the API returns
409 and the UI offers a reload; it cannot silently skip or duplicate records.

The same endpoint with `transaction` returns that wallet's saved normalized and
raw source record. The read path makes no provider calls. Transaction inspection
shows outcome, finality, block/slot and index, token labels and addresses, signed
received quantities, quote amounts, counterparties, network fees, source notes,
provider errors and raw evidence. Missing timestamps are unknown, not invented
collection times. Received quantities and fees retain their full decimal strings
inside the expanded evidence view. Every record is accessible through pagination;
closed positions remain accessible through the existing asset pagination.

For a classified, complete, verified single-target swap:

- Effective quote unit price = `abs(received quote flow) / abs(token flow)`.
- Estimated USD unit price = effective quote unit price × historical quote/USD
  candle. Today’s token price is never substituted.
- Network fee quantity comes from transaction evidence. Estimated fee USD uses
  historical native/USD evidence and is shown separately.

The effective price is a **wallet-flow ratio**. Costs already included in those
movements remain included; it is not a decoded protocol fill-price assertion.
Transfers, failed executions, provisional records and ambiguous multi-asset flows
have no verified entry or exit price. Their observed movements remain inspectable.
Incomplete RH/BNB traces or missing token denominations preserve validated receipt
and fee evidence without asserting complete native movements or inventing units.
Missing or conflicting transaction/receipt identities still fail validation.

Historical GeckoTerminal quote/fee pricing now has priority and runs independently
of the background HTTP pause. The live check received a real Gecko rate limit.
Public Kraken USD candles were separately verified for **SOL/USD, ETH/USD and
BNB/USD** and added as a labelled native-coin conversion fallback. Only exact
native/wrapped-native aliases are eligible. Hourly candles cover up to 720 recent
periods; daily candles cover older history. The API's final unfinished candle is
excluded. Each conversion records its source and hour/day granularity. This is an
indicative USD estimate, not an onchain fill or an assumed stablecoin peg.

The real SOL buy sample returned 21,843.230347 tokens and an effective quote price
of 0.0000000266296692732487256776 SOL/token. The fallback USD estimate was
approximately $0.00000322432/token; the received fee was 0.000006625 SOL. A second
SOL sale and an RH partial sale also returned sourced USD conversions and fees.
These sample executions do not establish whole-wallet profitability.

## Setup and deployment

Keep the existing Railway persistent volume and `WATER_TRACKER_DB_PATH`. The
schema change is additive; existing wallet/record/cursor/credit evidence remains.
There is no new database, queue, external subscription or required secret.

| Variable | Default / action |
| --- | --- |
| `WATER_TRACKER_CURRENT_DAILY_REQUESTS` | Optional, 24,000 current HTTP attempts/day; range 100–96,000. This is a local bound, not extra provider credits. |
| `WATER_TRACKER_DAILY_REQUESTS` | Existing, 2,000 background attempts/day. |
| `WATER_TRACKER_REFRESH_SECONDS` | Default changes to 360. If Railway explicitly sets 2,400, change it to 360 for the new adaptive cadence. The observed old value does not prove an explicit variable exists. |
| `WATER_TRACKER_HELIUS_CREDITS_31D` | Existing, 800,000; retain the shared project budget. |
| `HELIUS_API_KEYS` / `HELIUS_API_KEY` | Existing comma list/single key; no new Helius key needed. |
| `WATER_TRACKER_COHORT_LIMIT` | Existing default twelve; this change improves evidence for the cohort and does not silently expand it. |
| `WATER_TRACKER_RECORD_LIMIT` | Existing default 10,000 per wallet. Reaching it retains data/cursors and explains the stop; adjust explicitly if a larger evidence store is intended. |

Deploy core and web together. Optional fields remain backward compatible and the
web preserves legacy saved activity if the new endpoint is temporarily unavailable.
After deployment, inspect `/v1/wallets` for independent current allocations and
advancing head times, then inspect a wallet's balance read time, token mark time
and source record. The background counter may correctly remain paused that day.
Enrichment has a backlog after the first deployment; missing marks do not become
zero while it catches up. No production settings or storage were modified during
this development pass.

## Verification and remaining limits

123 Rust tests pass, including weighted/atomic budgets and rolling persistence, legacy spending,
a current Solana balance refresh during a background pause, quote-vs-current
pricing separation, native-candle pair validation/unfinished-bar exclusion,
missing/zero/stale values, exact Decimal prices, fungible metadata, raw record
lookup and exhaustive paging with changed-evidence cursor rejection. Existing
chain/RPC/ledger/scan regressions remain required. Next's production build validates
the web types and routes.

A controlled browser replay used all twelve real production analyses, newly
received public marks and the five representative source transactions at 320,
390, 768 and 1,440 pixels. It passed keyboard expansion, raw record inspection,
source/fee rendering and zero horizontal overflow. A separate controlled status
case checked the background-only pause message. This is a local replay, not proof
of a production deployment or complete history. In-session screenshots and full
received data are in `/workspace/water-detail-research`.

| Before | After | Why |
| --- | --- | --- |
| Twelve activity rows, mostly addresses, quantities or blank amounts | Pageable saved transactions, expandable movements, quote amounts, effective entry/exit prices, fees and source records | Received evidence can be inspected without inventing missing executions. |
| Contract addresses with unexplained value dashes | Received names/tickers and valuation quantity, mark, read times, source and explicit gap | Users can distinguish current balance evidence, incomplete reconstruction and missing pricing. |
| One daily pause blocked all work | Separate current, background and market work with provider guards | Backfill cannot spend the current allocation. |
| Prices hidden behind completeness checks | Individual received trade amounts stay visible while qualification remains gated | Incomplete history can still supply useful observations without a false profit rank. |

Fomo permitted discovery access was not configured in the audited deployment;
no complete Fomo private trade history is claimed. BNB still uses received token
identities for public log scanning and cannot prove native-only, failed or
unobserved-token history. A live attempt at address-independent wallet topic logs
returned the provider's `-32005 limit exceeded`; it was not adopted as a proven
replacement. The tokenless BNB candidate cannot be labelled fresh from only a
chain check. Unsupported contracts, unavailable traces, unindexed markets,
missing lots and short records still prevent profitability qualification. This
change makes those facts visible; it does not certify twelve profitable wallets.

## Primary sources checked during research

- [Helius billing and credits](https://www.helius.dev/docs/billing/credits)
- [Helius indexed wallet history](https://www.helius.dev/docs/rpc/gettransactionsforaddress)
- [Helius fungible asset batch metadata](https://www.helius.dev/docs/api-reference/das/getassetbatch)
- [Helius Parsed Events](https://www.helius.dev/docs/parsed-events) and
  [Parsed Streams](https://www.helius.dev/docs/parsed-streams): current free-plan
  options investigated for future event delivery. A stream requires reconnect,
  finality and backfill handling and was not pretended to be implemented here.
- [GeckoTerminal public API](https://api.geckoterminal.com/docs/index.html) and
  [public API limits](https://apiguide.geckoterminal.com/faq)
- [Dexscreener API reference](https://docs.dexscreener.com/api/reference)
- [Kraken public OHLC contract](https://docs.kraken.com/api-reference/market-data/get-ohlc-data)

Provider rows, RPC bodies and sample candle responses were received directly in
addition to reading these documents. Unsupported paths were recorded as failures.
