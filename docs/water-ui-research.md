# Water research workspace redesign

Research and captures: 5 October 2026. Implementation branch:
`feat/water-research-ui`. This is a UI change; wallet accounting, provider
budgets, qualification rules and stored evidence are unchanged.

## Observed flow and limits

1. **Scanner entry — usable, too much introduction.** Fresh captures of the
   production page at 390 and 1440px show a large marketing headline ahead of
   the lookup. The submit label “Read position” does not clearly describe a
   token scan. The page correctly exposes the failed health check.
2. **Wallet entry — API blocked.** Fresh captures show an API error alongside
   “Waiting for wallet history” and text suggesting collection is continuing.
   A failed response cannot establish that the cohort is empty or collecting.
   The large heading also pushes filters and results below the initial viewport.
3. **Populated scanner and wallet details — live audit blocked.** During these
   captures Water's API was unavailable. The user reported a restart, but the
   captured frontend still reported core/wallet unavailability. This pass does
   not identify a new crash cause or certify restored collection.

Accepted fresh captures and comparator screens are retained in
`/workspace/water-ui-research/`. Earlier internal screenshots are not audit
evidence. Populated implementation checks use explicitly controlled replays of
previously received production scan responses, wallet analyses, public token
marks and five re-fetched transactions; their original timestamps are retained.
These checks validate presentation and interactions, not current prices,
full history, or production availability.

## Primary research

| Source inspected | Observed organization | Applied to Water |
| --- | --- | --- |
| [DEX Screener Solana](https://dexscreener.com/solana), live browser capture | Compact controls immediately precede aligned token rows; identity, market cap, price, activity, volume and liquidity are separate columns. | Short page heading, aligned wallet rows, explicit field labels, search/filter/sort controls adjacent to results. |
| [GeckoTerminal Solana pools](https://www.geckoterminal.com/solana/pools), live browser capture | Network context and summary precede filters and pool metrics; pool identity remains separate from market and activity fields. | Keep chain/address identity and market summary visible above research views. Preserve the source/pool basis instead of merging unlike market reads. |
| [GMGN wallet detail documentation](https://docs.gmgn.ai/index/wallet-detail-page) | Wallet identity, address actions, holdings, activity and time-bounded performance serve different tasks. | Address copy/explorer actions and Activity, Positions, Performance and Evidence views. Keep Water's actual two-window assessment rather than adding unsupported period switches. |
| [GMGN token documentation](https://docs.gmgn.ai/index/token-page-chart-multicharts-activity-trading-system) | Token research separates activity, holders and pool/token information. | Overview, Holders, Origin and Sources & history views, with independent deep reads loaded on first use. |
| [Nansen Profiler 101](https://academy.nansen.ai/en/help/articles/0002013-nansen-profiler-101) | Address overview, transactions, counterparties and performance are distinct; token profiling focuses on balances and entry/exit behavior. | Keep financial summaries distinct from transaction movements and evidence. Preserve counterparties and original records inside expandable transactions. |
| [Nansen transaction research workflow](https://academy.nansen.ai/en/articles/8002716-use-transaction-history-and-reverse-engineer-entry-points) | Transaction times and token filters provide primary behavior evidence; holdings and PnL provide supporting context. | Default wallet detail to activity, retain timestamps, exact trade prices/fees and source-record inspection; add token/ticker/address search in positions. |
| [DEX Screener token listing](https://docs.dexscreener.com/token-listing) | Market cap depends on supply information and can differ from fully diluted valuation. | Preserve Water's `market_cap_basis`; supply-implied values remain “Supply value,” and missing data remains unavailable. |

The live GMGN tracking shell was inspected, but its populated tracking panes
require login. Its official documentation supplies the wallet-detail comparison;
this is not a claim that logged-in activity or profitability was audited.
No paid subscription, account creation or private wallet history was used.
These sources inform organization, not Water's market values or calculations.

## Information hierarchy

| User question | Visible first | Deeper evidence |
| --- | --- | --- |
| Which token and chain am I inspecting? | Symbol/name, chain, contract, copy and received social links | Origin, factory and creator evidence |
| What market data was received? | Price, liquidity, market cap/supply value, 24h volume, scan time and source count | Market basis, chain verification and complete source ledger |
| What explains the current structure? | Existing exit-pressure components, opposing observations and demand fields | Alternative explanations; current large holders and their supported economics |
| Which wallet should I inspect? | Address/source, history status, saved/pending counts, latest activity and last check | Separate per-month performance and qualification gates |
| What did it buy or sell? | Activity type, token, time, quantity and explicitly estimated USD unit price | Exact quote amounts/unit prices, fees, movements, counterparties, finality and raw source record |
| What does it hold and what is it worth? | Token, quantity, sourced current USD value, basis coverage and supported average entry | Balance/price source, observation time, block/slot and unavailable-value reason |
| Is this wallet's performance proved? | History/status stays visible throughout the profile | Performance includes recorded wins/losses, active days, fees, realized/open results and every qualification check |

No new aggregate portfolio value, return, win rate, price chart or wallet badge is
invented. All new values are existing received fields or record counts. Missing
market data says “Unavailable”; a received zero remains zero. Historical USD
trade conversions say “Est.” and remain separate from current position values.
Unknown token names still fall back to their addresses.

## Implementation decisions

- One shared header and visual language for both tools: charcoal surfaces,
  restrained moss/clay, tabular numeric text and structural borders.
- A compact scanner lookup replaces the marketing hero. Main market metrics
  remain visible while research views change.
- Wallets support address/source search, chain/discovery filters and sorting by
  last check or saved record count. Qualification remains an independent view.
- On desktop an open wallet appears beside its collection; on phones the detail
  replaces the list, and closing it restores focus to the opening wallet.
- Profile views preserve their state and data when switched. Holder/origin
  panels mount on first use and remain mounted afterwards, preventing repeated
  reads merely from switching tabs. A different token resets those views.
- Arrow keys, Home and End move research tabs. Every tab has a unique linked
  panel; hidden panels are removed from the accessibility tree.
- Position value/balance provenance moves into a labelled disclosure. The row
  keeps quantity/value/coverage visible while exact source evidence is accessible.
- Service failure is separate from empty collection. A retry action replaces
  unsupported claims about collection continuing; previous successful data is
  retained during refresh errors.
- Clipboard feedback reports success only after the browser accepts the write.
  Wallet opening is instant; reduced-motion preferences retain static feedback.

## Before / after review

| Severity | Location | Before | After | Why |
| --- | --- | --- | --- | --- |
| High | `web/components/wallet-tracker.tsx` | API failure and empty-history/collecting copy displayed together. | Explicit service-unavailable state and retry action, with no claim about saved cohort contents. | Distinguish missing response from received empty data. |
| Medium | Scanner and wallet entry | Large introductory headings and repeated instructional copy push controls down. | Compact task headings with lookup/filter controls immediately below. | Put the repeated task first. |
| Medium | Wallet detail | Activity, two months, coverage, positions and discovery form one long page. | Persistent profile summary with focused views and provenance disclosures. | Reduce scanning effort while retaining evidence. |
| Medium | Scanner results | Independent deep reads start for every result and all sections occupy one page. | Persistent market summary and lazy first-use holder/origin views. | Reduce initial provider work and make the user's research task explicit. |
| Medium | Wallet mobile rows | Dates and counts have weak local context after column headings disappear. | Mobile field labels, readable reflow, search and sorting. | Make each value understandable without desktop columns. |
| Low | Headers / addresses | Navigation differs slightly and address copying is absent. | Shared navigation and accessible clipboard actions. | Consistency and fewer steps for onchain verification. |

## Verification

Production-build browser checks cover 320, 390, 768 and 1440px; received-data
replay, unknown and zero market values, API failure, wallet/asset search,
chain filters, keyboard tabs, focus return, clipboard, transaction source
inspection and first-use holder/origin requests. Exact results and screenshots
are retained with the local verification artifact. The Next production build
passes. Both the wallet-evidence and scanner-evidence automated accessibility
checks at 390px report zero violations in their tested WCAG A/AA scope; they do
not certify full WCAG compliance.
Physical-device, full screen-reader and populated production-flow tests are
not verified while the API is unavailable.

This PR changes web presentation only and needs no new variable or service.
The backend deployment must respond before the new layout can show current
production data. Merging this redesign alone does not repair the Railway crash.

Approve the verified UI interaction and layout scope. Populated production
availability remains unverified.
