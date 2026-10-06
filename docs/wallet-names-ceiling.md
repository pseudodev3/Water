# Followed-wallet names and upper collection limit

Checked 2026-10-06. Personal names belong to a chain and execution address, not
to a verified identity. Follow a wallet, then use its pencil button in the list
or profile. Save a name of up to 64 characters; leave it blank to restore address
display. Names appear in the wallet list, profile and Following activity, and
search matches either name or address. Canonical addresses, explorer links and
copied addresses remain unchanged.

Names are local to the browser, like follows. The existing
`water:followed-wallets:v1` array is not migrated or rewritten by renaming.
`water:wallet-names:v1` stores a map keyed by `chain:wallet`. Unfollowing hides
the name but retains it for refollowing. Failed writes leave the old map and
follow list intact; malformed name storage does not prevent reading follows.
The editor uses a native modal dialog, labelled input, Enter to save, Escape or
Cancel to dismiss, and focus restoration. Names render as escaped text.

## Collection ceiling

`WATER_TRACKER_MAX_WALLET_USD` defaults to `50000`, alongside the existing
`WATER_TRACKER_MIN_WALLET_USD=1000`. Neither requires an environment change.
Each value uses fresh received native and token balances and USD marks on the
selected chain. It excludes off-wallet DeFi and NFTs whose valuations are unproved.

A fresh priced total **or lower bound above $50,000** is `above_maximum`.
It pauses expensive current history, backfill, transaction reconstruction and
historical pricing, and schedules a bounded balance check in six hours. The
wallet remains in storage with its original records, losses and cursors. Value
checks exposes it as **Above ceiling · paused**. Exactly $50,000 is admitted.
Falling back within the range can resume collection through existing admission
and wake-up paths. Active capacity and shared provider budgets still apply.

A partial $10,000 value cannot prove a total under $50,000. Incomplete wallets
whose received lower bound meets $1,000 remain candidates, labelled **Upper
limit unverified**. This is deliberate evidence-based exclusion: it does not
silently claim a complete under-ceiling portfolio or treat missing prices as zero.
Wallet balance alone also does not establish motivation or profitability. The
existing strict 30/60-day qualification checks are unchanged.

With the ceiling enabled, native balances within the range no longer skip token
screening. Solana inspects both owner token programs. BNB/RH inspect up to four
known token contracts at a received finalized block; they cannot claim full
inventory coverage. Native holdings already above the ceiling need no token
requests. Shared reservation, retry and request-budget safeguards remain in place.

Set the maximum to `0` to disable only the ceiling; set the minimum to `0` to
disable only the floor. Both zero disables capital screening. Configure the
ceiling at least as high as the floor for a nonempty admitted range.

## Verification

140 Rust tests pass, including native-plus-token inclusive boundaries, fractional
over-limit amounts, partial/stale/unknown handling, zero-disabled settings,
actual RPC/request reservations on all three chains, skipped token census for
over-limit native balances, complete boundary recovery, source retention and
active/history cohort exclusion. Older persisted WalletValue objects remain
readable because the added maximum field defaults to missing on deserialization.

The Next production build and received-evidence browser replay pass at
320/390/1440px. Checks cover names across list/profile/feed, search, reload,
canonical addresses, Enter/Escape/Cancel/focus, 64-character names, escaped
markup, unfollow/refollow, blank resets, corrupted storage and induced write
failure. Ceiling membership and unverified upper-limit labels are checked with
explicitly controlled eligibility values. Axe reports zero WCAG 2 A/AA and 2.1 AA
violations on rename dialogs. No browser errors or page overflow were found.

Session evidence: `/workspace/water-wallet-name-research`. Deployment/live
verification is separate from replay; check the merged commit's statuses and
actual production API/UI before stating availability.
