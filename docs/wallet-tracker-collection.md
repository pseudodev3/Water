# Wallet collection and stale evidence

Checked on 2026-10-05 against the production wallet page and Railway API.
This follow-up extends PR #24; it is not deployed until merged.

## Observed production behavior

At 05:50 UTC, `/v1/wallets` reported collection enabled, storage configured and
**2,000 / 2,000** daily collection requests used. The newest saved wallet check
was **02:24:32 UTC**. All 12 collected wallets were stale under the existing
one-hour rule. The API explicitly reported that collection had paused until
UTC midnight. This establishes Water's daily limit as the reason collection
was paused at that check; it does not establish when the last request was spent.

Helius's separate conservative credit counter was 8,980 / 800,000 over 31 days.
The daily HTTP limit and the Helius credit limit are distinct. Adding credentials
does not raise either shared limit.

The three measured production detail requests returned HTTP 200 in 0.499,
0.513 and 0.569 seconds. A production browser request also opened a wallet in
0.327 seconds. No spontaneous server timeout was reproduced. With a controlled
13-second response delay, the actual production client instead aborted after
12.459 seconds. That establishes its short client deadline, not the cause of
the user's connection delay. A detail request reads stored evidence; it does
not collect new external history on demand.

The default qualified view contained no wallets. Research rows hid saved record
counts behind "Awaiting verification", and useful activity followed both full
month-check lists. Captured research data includes actual received transfers and
positions, but does not establish a qualified profitable cohort.

## Collection changes

Current history checks now have priority and a separate fair due schedule.
The default target is **40 minutes per wallet**. One head page is committed
and analyzed independently of historical backfill, transaction retries, balance
reads and pricing. Failed reads do not advance freshness. A BNB nomination
without known tokens receives only a chain check; that cannot refresh its
wallet-history timestamp or mark its head complete.

The existing default daily ceiling stays 2,000 attempted HTTP requests:

| Purpose | Daily allocation | Behavior |
| --- | ---: | --- |
| Current history | 1,400 | Priority; one due wallet per collector tick |
| Historical reconstruction and state | 500 | Initial allowance of 125, remainder released over the UTC day; normally wait for at least 25 available requests before starting a pass |
| Candidate discovery | 100 | Runs when the cohort has space, at most once per six hours |

These are ceilings, not a promise to spend every request. All calls still reserve
against the same persisted global counter before sending. Failed attempts count;
rejected reservations do not. The Helius rolling credit reservation is atomic
with that accounting. Existing spending is not reset by this upgrade. In
particular, a deployment after today's 2,000 requests are used must wait for
the next UTC day. Unused allocations are not borrowed from current checks.

Historical pages, reconstructed records and received state are committed and
analyzed as they arrive. A 30-second pass limit preserves completed work. Price
collection continues under GeckoTerminal's existing shared rate limiter, with
six starts reserved for scans and at most 18 optional starts per minute; those
market requests are outside the tracker RPC/index/discovery HTTP counter.

Account/balance state has its own `last_state_checked_at` timestamp. A new head
page cannot make old state fresh. Qualification requires both current history
and account/balance state checked within one hour, alongside the existing full
history, economics and consistency gates. Old stored coverage defaults this
new field to unknown and needs a new state check. Token-overlap results apply
the same freshness demotion.

The status API adds collection state, UTC budget reset, current target interval,
per-purpose allocation/availability and a separate historical-work error.
Availability respects the existing global counter, including spending incurred
before this upgrade. A cadence is a target; unavailable sources, record caps,
provider quotas and missing BNB token identities can still leave evidence stale
or incomplete. This does not certify complete 30/60-day history or profitability.

## Page and deadline changes

The page opens the research list, shows saved records, pending reconstruction
and the latest known execution date, and explains a budget pause while keeping
observations accessible. Wallet details show observed activity first. The full
qualification checks remain available in keyboard-accessible disclosures.
Current-history and account/balance check dates are displayed separately.

Wallet requests now allow 30 seconds. Refreshing an already open wallet retains
its saved details while waiting and if the refresh fails or times out. Filters,
follows, links and qualification thresholds retain their existing behavior.

No new required variable, paid plan, storage service or credential is needed.
Optional `WATER_TRACKER_REFRESH_SECONDS=2400` controls the head-check target
(300–3,000 seconds). `WATER_TRACKER_INTERVAL_SECONDS=60` remains the worker tick.
The configured daily and Helius limits remain in force. A smaller interval or
larger cohort can consume the current allocation and does not override it.

## Validation

All 114 Rust tests and the Rust build passed. Regressions cover current checks with exhausted history allocation,
independent state freshness, tokenless BNB freshness, atomic global/credit
accounting, legacy spent counters, UTC pacing and fair due times. A controlled
24-hour budget simulation for the captured cohort's normal head routes uses
1,116 current requests and cannot let history spend the 1,400 current allocation.
This is a simulation, not a 24-hour production observation or a provider latency
guarantee. Provider failover can cost extra requests and remains bounded.

The web production build passed. Browser checks using captured real production
data passed at 320, 390, 768 and 1,440 pixels, without horizontal overflow or
uncaught page errors. They cover default research visibility, received activity,
keyboard disclosure controls, persistent follows and chain filters. The same
controlled 13-second delay now returns a detail successfully in 13.415 seconds.
A stalled refresh aborts after 30.456 seconds and retains the open saved detail.

Evidence: [collection checks](research/wallet-tracker/collection-checks.json).
Earlier provider fixes and remaining social-feed gaps:
[RPC and Pump coverage](wallet-tracker-rpc-pump.md).
