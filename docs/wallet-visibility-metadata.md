# Wallet visibility and scanner metadata — 2026-10-05

## Received evidence

The screenshot's wallet is
`8deJ9xeUvXSJwicYptA9mHsU2rN2pDx37KWzkDkEXhU6` on Solana. Its captured
profile contained 2,958 saved records and 1,023 positive-balance holdings, with
no fresh positive USD valuations. A 100-record activity sample contained repeated
10-token transfers of `HCESMKvzY7D1XbsyNJz1Gvmahot4gQuG3aLQno68pump`.
Received token name/symbol advertised a data-stream service. The transfers had
no swap evidence, complete received movements, zero wallet-paid fees and no
received USD price. This is a reproducible visibility decision, not a general
spam/scam classifier. Unknown marks do not prove a token worthless.

## Wallet defaults

- Positions requires a positive token quantity and a positive received USD
  valuation. There is no dollar minimum or dust threshold. Freshness remains
  the existing 15-minute market-mark limit; received RPC quantities override
  reconstructed historical quantities.
- Unpriced and zero-value positive holdings are hidden by default. The checkbox
  restores them for inspection. Confirmed zero/negative quantities stay hidden.
  Empty states distinguish no collected balances from no positively valued
  holdings. The metric and summary count describe valued positions.
- Summary counts use the same effective quantity, market mark, freshness and
  checked decimal multiplication as detail. `unpriced_positions_count` records
  the remaining positive holdings. A mark expiring can remove a holding from
  the default view without deleting its balance or history.
- Activity hides only successful, finalized, complete token-only transfer
  records with neither received historical movement value nor a fresh positive
  spot mark. Historical prices and current marks are distinct: a spot mark
  only controls visibility and never supplies historical entry/exit economics.
- Trades, native/stablecoin funding, failed/provisional/unresolved transactions
  and fee-only records remain visible. Mixed transactions stay visible if any
  meaningful movement remains; filtering does not erase individual movements.
- Filtering happens before pagination. `/v1/wallets/activity` accepts
  `include_unvalued` (default false) and returns `total` visible records,
  `saved_total`, `hidden_count` and `include_unvalued`. The checkbox requests
  all saved records and restarts pagination. Continuations are bound to saved
  evidence, filter mode and visible record membership, rejecting changed
  membership instead of skipping rows.
- Source inspection by transaction ID bypasses visibility filtering. The stored
  ledger, records, historical activity, FIFO basis, losses, fees, monthly windows
  and qualification gates remain unchanged. Summary/followed activity is a
  filtered read-time projection. Overlong provider names/symbols use the address
  in compact wallet labels; original metadata remains in returned evidence.

## Scanner images and socials

`/v1/token-info` now uses the market-provider adapter while remaining independent
of `/v1/scan`. GeckoTerminal metadata stays primary. If image, websites or socials
are missing, the adapter fills available missing fields from Dexscreener's
deepest indexed pool for the exact chain and **base** token. A quote-token or
different-chain match cannot supply metadata. Solana retains base58 case; EVM
address comparison is case-insensitive. The received Gecko address is checked.

The response keeps existing fields and adds `socials` and a source ledger.
Supported supplied links include X, Telegram, Discord, Farcaster, Zora,
Instagram, TikTok, YouTube and Reddit. Only parsed HTTP(S) URLs without embedded
credentials render. Provider links do not verify project ownership; Dexscreener
cannot set GeckoTerminal verification. Received primary values are preserved.

The independent cache coalesces simultaneous requests, caps entries at 256 and
keeps complete metadata for ten minutes; incomplete results retry after 30
seconds. Each provider has a four-second budget. Existing Gecko cooldown and
reserved main-scan allowance still apply. Metadata failure returns explicit
missing fields/source failures while the scan remains usable. Images have a
fixed footprint, real alt text, no-referrer requests and a missing-image state.
The scanner displays the supported returned links without the old five-link
cutoff. No paid access or new configuration is required.

Primary sources checked:

- [GeckoTerminal API documentation](https://api.geckoterminal.com/docs/index.html)
  and actual `/networks/{network}/tokens/{address}/info` responses.
- [Dexscreener API reference](https://docs.dexscreener.com/api/reference) and
  actual `/token-pairs/v1/{chain}/{address}` responses.

## Verification

- 132 Rust tests pass. New checks cover value/mark freshness and received balance
  precedence on all three chains, tiny positive values, transfer filtering before
  paging, unknown native funding, unresolved/failed/fee-only preservation,
  direct hidden-source inspection, cursor changes, unchanged accounting,
  exact-chain/base metadata, unsafe links, coalescing/cooldown fallback and a
  healthy real HTTP primary adapter path.
- A read-only replay of the 100 captured transactions uses their received
  normalized movements, original prices and metadata. The default view returns
  74 records and hides 26; the advertising mint disappears. The all-records view
  returns all 100. Snapshot and complete accounting serialization stay identical.
  This sample is not a full-wallet history completeness claim.
- Production web build and nine browser cases pass at 320/390/1440px. Received
  wallet data checks pagination, filtering, all-records toggles, original raw
  source inspection and preserved performance. Explicit controlled edge inputs
  check positive/tiny values, zero USD marks, missing prices and zero balances.
  Scanner cases use received metadata and actual downloaded provider images,
  with separate controlled missing-metadata and broken-image cases. No JS
  exceptions or horizontal overflow; scoped WCAG A/AA checks pass.
- Real new Rust metadata reads return images for Todd/Solana, wrapped ETH/RH and
  wrapped BNB. Todd returns Website, X, Telegram and Discord. RH/BNB samples
  return no socials: no links are fabricated. A controlled local Gecko 429 with
  **live** Dexscreener data returns Todd's image, two websites and five socials,
  including Instagram and TikTok. This proves fallback under induced failure,
  not universal metadata coverage.

Production merge/deployment verification is performed after the PR checks pass.
Collection/storage limits and provider history gaps are unchanged.
