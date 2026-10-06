# Wallet capital screening and Fomo design research

This records PR #31's minimum-only policy. The later $50,000 ceiling changes
native-only admission; see [wallet names and upper limit](wallet-names-ceiling.md).

Checked 2026-10-05. This change requires at least $1,000 in received native and
wallet token USD holdings before spending on current history, backfill,
transaction reconstruction or historical pricing. The user selected total
holdings, including SOL/ETH/BNB. The value applies to the selected chain; this
is not a cross-chain account aggregation or a DeFi/NFT valuation service.

## Value policy and request savings

The native balance is read first. A fresh native balance and USD price alone
at or above the floor admit the wallet without a token census. Otherwise,
Solana queries both the Token and Token-2022 owner programs and aggregates all
received mint quantities, including wrapped SOL and stablecoins. Native SOL and
wrapped SOL are distinct balances. Successful finalized reads retain their
actual response slots; the separate RPC responses are not an atomic snapshot.

BNB and RH verify chain 56/4663, get a finalized block and read native balance
at that actual block. Up to four already known token balances are screened
at the same block. These public RPCs cannot enumerate every EVM token owned
by an address. The received EVM token inventory stays partial. Unobserved tokens
may prevent admission; a small native balance cannot establish a low total.

USD arithmetic uses checked Decimal operations. A fresh known lower bound of
at least $1,000 is enough to admit; a low total requires a complete received
wallet inventory and prices for every positive holding. Balance age is at most
one hour and quote age at most 15 minutes. Missing, failed, expired or overflowing
values stay unknown and are never replaced by zero. A proved empty wallet is zero.
Partial totals display `≥`; unavailable totals display `Value pending`.

Confirmed low wallets sleep six hours between balance checks; unknown values
are checked at most hourly, eligible balances target 30 minutes. Worker cadence,
provider availability and existing request/credit ceilings still apply. The next
value-check schedule is reserved before requests, so a timeout cannot trigger an
unbounded immediate retry. Sleep schedules persist in SQLite. Shared market
batches prioritize native coins and valuable holdings; each inactive wallet
contributes at most 30 positive inventory assets, with bounded rotation. Historical
pricing is skipped for inactive wallets. Fresh prices that prove eligibility wake
sleeping current work. Market capacity stays six batches of 30 per pass, inside
Gecko's existing optional-request guard and scan reservation.

Saved records, loss accounting, prices, continuations, follows and candidates are
retained. Value checks exposes inactive wallets and their received evidence.
This is a collection allocation policy, not a proof of profit or a claim of
complete trading history. All existing 30/60-day qualification gates still apply.

The default active cohort is 12 wallets, selected by greatest established USD
lower bound. At most 48 candidates occupy its screening pool. Candidates that
meet the floor can wait for cohort capacity; the floor does not promise immediate
history work. Automatic discovery can run while active capacity and screening
space remain. This bounded sample is not an exhaustive profitable-wallet search.
Background and current request ceilings, shared rolling Helius credits and public
nomination restrictions remain in force; no additional keys or paid APIs are needed.

## Screenshot-based design audit

Evidence was captured before editing:

1. Water's live wallet list at 1440px: values were absent from the wallet list,
   record/status labels competed for attention, and generic bordered summaries
   dominated the page. Health: functional, with weak financial hierarchy.
2. Water's live initial scanner at 390px: the form and tools were reachable,
   but narrow header navigation and small explanatory labels made orientation
   harder on a phone. Health: usable, with room for clearer hierarchy and touch access.
3. Fomo's public home page loaded HTTP 200 in Chromium. Its official current
   navigation guide and profile/leaderboard/desktop images were inspected. No
   authenticated Fomo account or trading interaction was accessed.

Primary sources:
- [Fomo public site](https://fomo.family/)
- [Official navigation guide](https://fomo.family/blog/learn/navigating-your-fomo-app)
- [Official profile image](https://fomo.family/images/blog/learn/navigating-your-fomo-app/profile.webp)
- [Official leaderboard image](https://fomo.family/images/blog/learn/navigating-your-fomo-app/friends-and-leaderboard.webp)
- [Official desktop image](https://fomo.family/images/landing/fomo-desktop.webp)
- [November 2025 design recap](https://fomo.family/blog/november-2025-recap)

The profile image gives the USD total the strongest hierarchy, then cash and
open positions; token amounts are secondary. The leaderboard pairs a compact
identity with a right-aligned financial result in consistently spaced rows.
The desktop image uses focused search, tabs and separated market summaries.
Fomo's published redesign describes a darker interface and glass navigation.
These observations justify specific layout choices; public images do not prove
how its loading, error, keyboard or accessibility states behave.

Water now uses quieter neutral surfaces, a restrained periwinkle navigation
accent, clearer USD hierarchy, compact chain identities, consistent row spacing,
and a prominent total in profiles. Mobile tool navigation remains fixed to the
viewport with safe-area clearance and 52px targets. Wallet views scroll when
necessary, and an empty Discover view links directly to Value checks. On phones,
search stays visible and a Filters control reveals chain, source and sort, reducing
the distance to the actual wallet values. Desktop filters remain visible. The scanner
shares the palette, focused headline and clearer primary action. Existing token
images/socials and detailed evidence remain in their respective views.

Water's balances and evidence are insufficient for a verified portfolio chart
or arbitrary wallet profile photo. Neither was fabricated for the redesign.
The chain badge identifies a chain, not a trader. Invalid/missing market data
keeps its explicit state instead of becoming a decorative performance number.

## Verification and limits

The full Rust suite passes 137 tests. Five capital-policy regressions cover
native-plus-token exact arithmetic, unknown/stale handling, zero inventory,
actual RPC reservations, persistent source retention, restart persistence,
exact $1,000 native-only admission, verified BNB/RH finalized native and token reads,
wrong-chain rejection, active cohort capacity and history selection that skips
inactive candidates. A complete low Solana mock uses three balance requests
and zero history requests; repeating before the scheduled check uses none.

The Next production build passes. Browser checks at 320/390/768/1440px replay
received production evidence with explicitly controlled eligibility states:
$12,500/$1,000 sufficient lower bounds, $995 complete low total, $500 partial
unknown, and fully missing values. These totals are test fixtures, not live wallet
values. Checks cover high-value sorting, cutoff membership, inspectable value
checks, saved follows, profile totals, holdings sort, keyboard tabs, scanner tool
navigation and a safe rollout with no established values. No page overflow or
JavaScript errors were found. Received scanner image and all social-link rendering
also pass at 390/1440px. Scanner results and wallet profiles reserve scroll clearance
so the sticky header leaves token/wallet identity visible. WCAG 2 A/AA and 2.1 AA axe checks on phone/desktop
profiles report zero violations; this is automated coverage, not a full accessibility
certification. Screenshots were inspected and used to fix an ancestor blur that
incorrectly positioned mobile navigation.

Session evidence is stored under `/workspace/water-fomo-research`: original
reference images, before captures, viewport screenshots, browser-verification.json,
and Rust/build logs. Production rollout verification remains separate from replay fixtures; check
the merged commit’s Vercel/Railway statuses and live health before claiming availability. Source completeness and profitable track-record qualification
still depend on provider coverage and economic reconstruction; there is no measured
completion ETA from this UI or capital gate.
