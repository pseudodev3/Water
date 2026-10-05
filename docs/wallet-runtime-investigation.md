# Wallet runtime investigation — 2026-10-05

The user's Railway logs show repeated volume mounts and fresh `Water core
listening on 8080` messages, sometimes 13–20 seconds apart. The excerpt contains
no exit code, panic, shutdown message or container memory limit. It proves
repeated starts; it does not establish their cause. The historical deployment
also reported a failed status and returned Railway 502 responses.

At 18:36 UTC `/health` returned 200 with `Access-Control-Allow-Origin: *`.
At 18:41 UTC `/v1/wallets` returned 200, 12 wallets, a largest recorded history of
4,069 transactions and a current-collection timeout message. PR #26's merged-head
Railway and Vercel statuses subsequently succeeded. These are availability samples,
not evidence of sustained recovery or the reason for the earlier terminations.

## Reproduced memory amplification

Previously every stored record's nested raw provider JSON was deserialized for
accounting and collection. Profile and activity reads cloned the entire snapshot;
accounting also cloned every normalized transaction. Collection retained older
snapshots while loading additional ones. Started Tokio blocking tasks continue
after an outer timeout, so that timeout alone could not bound their resource use.
See [Tokio's blocking task documentation](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).

A controlled SQLite fixture repeats one previously received Solana record 4,069
times with distinct replay identifiers. It matches an observed record count, but
is **not** the production wallet's history. It is used solely to measure code
resource use; it provides no evidence of wallet profitability. No provider calls
or production credentials were used in the replay.

Three fresh-process runs against the same already-built database produced:

| Debug build phase | Previous code | Revised code |
| --- | ---: | ---: |
| Loaded snapshot RSS | ~214 MiB | ~10 MiB |
| Profile accounting high-water RSS | ~420 MiB | ~15 MiB |
| End-to-end replay, including output serialization | 1.67–1.87 seconds | 0.65–0.79 seconds |

Output serialization raised revised whole-process peak RSS to approximately
18 MiB. All 4,069 accounting events and the complete serialized financial,
coverage and qualification output were identical. These numbers are local debug
measurements, not Railway resource readings or production performance promises.

## Change

- Snapshot reads skip parsed transactions' raw JSON allocations while retaining
  normalized transactions, errors, all record identities, prices and coverage.
  Unparsed references retain their ordering/source fields. SQLite source rows are
  neither changed nor removed by these reads.
- Accounting borrows snapshot and transaction data. Full source JSON is loaded
  for the selected transaction when inspecting evidence, and for the bounded
  retry set. Failed retries preserve previously received source evidence.
- Activity revisions hash persisted record bytes under the snapshot read lock.
  A raw-only evidence change still invalidates a cursor. Existing pre-deployment
  cursors require a first-page reload because the revision format changed.
- Two shared accounting slots bound profile/activity/nomination computations and
  background analysis. A started blocking task owns its slot until it finishes,
  even if the caller times out. Excess public computations return HTTP 503 with
  a retry message; saved data is not presented as an empty collection.
- Current collection starts/finishes/errors and history/enrichment failures or
  timeouts now emit application logs. SIGTERM and SIGINT emit a shutdown reason
  before draining HTTP requests.
- Linux emits `Water runtime resources` every ten seconds: process RSS/high-water,
  visible cgroup memory usage/limit and OOM-kill counter. A missing or unlimited
  cgroup limit is reported as unknown, not zero. The cgroup readings cover the
  visible group and may include other processes. Keep `water_core=info` enabled
  in `RUST_LOG` to see these readings and shutdown messages.

## Validation and limits

All 126 Rust tests pass, including accounting equivalence, pending reference
ordering fields, raw inspection, preservation after a failed retry, raw-only
cursor invalidation and unknown resource readings. The core and Next production
builds pass.

A local HTTP run against the same controlled database verified 4,069-record detail,
activity pagination without duplicates, raw source inspection, a 409 after a
raw-only update, and CORS on success/busy responses. Forty overlapping detail
requests returned two 200 responses and 38 bounded 503 responses. Process peak
RSS was ~36 MiB; health remained 200. SIGTERM logged its reason and exited with
code 0. This load test deliberately saturates the two computation slots; it does
not represent normal usage or verify every collector/provider combination.

The actual crash cause remains unconfirmed until the failed Railway deployment's
termination details and resource limit are available. A platform memory kill can
leave no application exception, but repeated startup lines alone cannot prove
one. Railway documents resource-limit/OOM diagnostics in its
[deployment troubleshooting guide](https://docs.railway.com/deployments/troubleshooting/slow-deployments)
and deployment exit/restart behavior in
[deployment actions](https://docs.railway.com/deployments/deployment-actions).
Do not reset the volume, erase transaction history or raise paid limits to obtain
evidence of an earlier crash.
