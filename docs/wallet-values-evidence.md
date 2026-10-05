# Wallet holding values and evidence readiness

## Positions

The large USD amount is quantity multiplied by the received current market mark.
The smaller explicitly labelled price is for one token; quantity remains separate.
This is a marked holding value, not a trading profit or guaranteed sale proceeds.

Holding totals normally show cents. Positive sub-cent totals use four significant
digits, with scientific notation below $0.000001, so a received positive mark
never appears as $0.00. Per-token prices retain their existing precision. Missing
data stays `Unavailable`.

Positions default to highest total USD value first. Lowest total value and recent
activity are available. Sorting applies before the display limit and search uses
the selected ordering. Missing/invalid values remain last in either value order;
received zero marks remain distinct from missing prices. Visibility filters,
transaction history, losses and qualification calculations are unchanged.

## Production evidence check

The received list snapshot at 21:53 UTC on 2026-10-05 contained 12
wallets, 19,685 saved records and no 30/60-day qualified wallet. Its aggregate
unresolved counter was 5,150; this counts accounting gaps and is not a count of
distinct failed transactions. Snapshots change as collection continues.

Background history/discovery was paused at 2,000 / 2,000 requests. Its next reset
was 2026-10-06 00:00 UTC (01:00 Africa/Lagos). Current collection remained enabled
under a separate 24,000-request allocation, with a reported 630-second target.
Helius's reserved-credit counter was 12,009 / 800,000 over 31 days and was not
the reported cause of this background pause. Resetting the daily budget resumes
attempts; it does not certify that all evidence will be complete that night.

Solana still had historical pages and economic gaps to reconcile. The small
`76CzYN…` wallet had complete indexed history, reconciled ending balances and 62
records, but still 12 economic gaps and no passing Complete history qualification
gate. Both RH accounts had finished index pagination but failed balance/account
and economic checks; `0x5ab1…` reported `Invalid hex amount.` on its latest state
read. No state-read root cause was inferred from that error alone.

BNB's public fallback indexes known-token transfers, not complete native-only,
failed, internal-only or previously unknown-token history. Complete 30/60-day
qualification remains unavailable on that route. Fomo permitted discovery access
was not configured in the snapshot. These are coverage gaps, not waiting periods.

Complete evidence requires historical backfill and current-head coverage,
canonical ordering, supported execution accounts, resolved executions, reconciled
balances, opening basis, historical USD conversions and fees. Qualification also
requires activity predating each 30-day window, positive realized and total
results, sample size and outlier checks, and current history/state freshness.
Two independent passing windows establish the 60-day record. Existing historical
data can be backfilled; the app does not inherently need to run for 60 new days.

No reliable completion date or percentage can be calculated from saved-record
counts because the remaining history and unsupported executions are not known.

## Validation

The Next production build and 132 Rust regression tests passed. Browser checks
replayed received production data with 245 positively valued holdings at 320,
390 and 1,440px. They verified sorting the full set before display limits, both
USD directions, recent activity, search and show-more. Separate controlled cases
verified sub-cent/scientific marks, zero balances, received zero USD and missing
values; unknowns remained last. No JavaScript errors or horizontal overflow were
observed, and the 320px WCAG AA check passed.
