# Helius budgets and BNB Chain

Merged and deployed through PR #23 at
`cd0fe21634153c30fdfe673173e4d231f53e547f`. Subsequent state-read fixes and Pump
social-feed limits are described in [RPC and Pump coverage](wallet-tracker-rpc-pump.md).

## Helius: start with one free key

Helius's current free plan includes **1M credits per project per credit cycle**,
not one million calls to every endpoint. Standard RPC calls cost one credit,
some archival methods cost ten, and `getTransactionsForAddress` costs ten per
100 returned full transactions, rounded up with a ten-credit minimum. Water
requests at most 100 full transactions per page. It does not use the legacy
100-credit Enhanced Transactions REST API.

The existing default is at most 2,000 tracker HTTP attempts per UTC day across
all sources. If every attempt were a ten-credit Helius call, 31 UTC days would
reserve 620,000 credits; a billing period touching 32 UTC date buckets would
reserve at most 640,000. Actual Helius use should be lower because many requests
go to other providers, but quota alone does not guarantee rapid backfill or
complete history for a high-activity wallet. Measure the initial 12-wallet cohort
before expanding it. Calls made by other apps/webhooks also use your project.

Use `HELIUS_API_KEY` for a single server key. Optional credential failover accepts
up to eight unique keys:

```dotenv
HELIUS_API_KEYS=first_key,second_key
WATER_TRACKER_HELIUS_CREDITS_31D=800000
```

The plural variable takes precedence when nonempty. Whitespace/empty entries
are removed and duplicate keys are ignored. A rejected credential (HTTP 401/403)
can fall through to the next key. Rate/quota HTTP 429 responses pause collection
for an hour; they do not switch keys around the quota. Keys for the same project
share its plan allowance. All configured keys share one Water limit regardless
of how many projects they belong to. No purchase or autoscaling is enabled.

Before every Helius attempt, Water atomically reserves ten estimated credits and
one daily HTTP attempt in SQLite. Failed calls and one-credit methods remain
charged at ten in this local estimate, which is deliberately conservative. The
800,000 default stop limit covers a rolling 31-day interval using the current
and preceding 31 UTC date buckets, including the entire partially overlapping
start bucket. Counters survive restarts and key changes. Collection retains its
cursors at the limit. The status/API and discovery disclosure show **Water's
reserved estimate**, not your provider's actual remaining balance. Check the
Helius usage dashboard for all project traffic and its actual credit-cycle dates.

## BNB Smart Chain mainnet

BNB uses API/storage identity `bnb`, chain ID **56**, market-provider slug `bsc`,
and native asset **BNB**. It is separate from RH 4663 and ETH even when the same
wallet address appears on both chains. Public token scans include GeckoTerminal
market data, exact-chain Dexscreener fallback, bytecode/chain verification and
ERC-20 supply. The current holder index and launch creator are unknown. Standard
`owner()` reads are reported as owner observations, not creator/control proof.

```dotenv
BNB_RPC_URL=https://bsc-rpc.publicnode.com
BNB_FALLBACK_RPC_URL=https://bsc-dataseed.bnbchain.org
BNB_TRACE_RPC_URL=https://bsc.drpc.org
```

The fallback must also identify as chain 56. Historical state/trace availability
is checked per transaction. Public-node denied reads, dRPC rate limits/missing
traces and official-node historical-state/log restrictions have all occurred
during acceptance; configuring a URL does not prove archive capability.

Automatic BNB nominations sample four contract calls from a finalized block.
Successful PancakeSwap V2 swap logs require the pinned Sourcify-verified runtime,
opposite input/output amounts, matching pool tokens, the documented factory and
that factory's `getPair` result at the transaction block. A token symbol, router
call or event topic alone does not establish execution. Discovery gives each
chain a turn before the bounded cohort fills; existing saved wallets are retained.

The V2 runtime is from factory-registered pool
`0x16b9a82891338f9bA80E2D6970FddA79D1eb0daE`. Sourcify reports an exact runtime
match for `PancakePair.sol:PancakePair`, solc `0.5.16+commit.9c3226ce`. Its
recompiled runtime body matches the live finalized bytecode. Only the compiler
CBOR metadata is removed; no opcode or storage/token identity is masked.
The checksum and provenance live in `core/src/tracker/bnb-v2-template.json`.

Public wallet observation follows token identities actually received in its
execution receipts (up to 64). Address-scoped inbound/outbound Transfer log
windows retain block continuations, merge adjacent scanned intervals and bridge
empty intervals without pretending an empty page ends history. Broad wallet log
queries are restricted by some free public nodes. The fallback can miss tokens
never received by Water, native-only funding/exits, failed calls and internal-only
activity. **It cannot qualify a complete BNB 30/60-day record.** No indexed native
history is inferred from ERC-20 logs or an address nonce.

Received receipts survive missing native traces or historical token precision.
Unknown denominations do not become normalized quantities; absent internal BNB
proceeds do not become zero. Such transactions appear as unresolved execution
with raw evidence retained and complete economics withheld. Partial records retry
after an hour. Standard gas costs use `gasUsed * effectiveGasPrice` in BNB; RH's
Arbitrum refund/payment rules are not applied to BNB. Zero-fee/sponsored account
models remain incomplete. BNB/USDT/USDC are quote assets, but only exact native
BNB/WBNB wrapping is neutral; equal quantities of unrelated stablecoins are not.

Wallet filters, explorer links, follows, saved token observations, scanner
prefills and qualified-overlap chain identity all recognize BNB. Other BNB venues,
Four.meme/Pancake V3 execution adapters, creator graphs, complete holder indexing
and wallet-wide archived history remain additional work. Helius provides Solana
data and does not supply BNB history.

## Free-history research

- Etherscan's current supported-chain matrix marks BNB mainnet 56 unavailable on
  its Free tier for normal history; ABI/source endpoints are available separately.
- Moralis's current public pricing starts at a paid Starter plan; it is not wired
  as a supposedly free runtime source.
- Routescan's chain-56 test returned `chain not supported`. Anonymous Blockscout
  chain-56 history returned HTTP 402; that does not prove a free usable index.
- NodeReal advertises a free plan, but its `nr_getTransactionByAddress` method
  documents external/ERC-20/ERC-721/ERC-1155 categories and 1,000-block intervals,
  while a migration guide also mentions internal transfers. A quota or terminal
  page from that API cannot establish missing native/internal history. No key is
  available for reconciling the contradiction or testing complete wallet coverage.

Before BNB qualification, validate a permitted free wallet-wide index, every
native/internal/failed route, historical state and opening balances on real
wallets. Keep BNB observations under research until those proofs exist.

## Sources checked October 4, 2026

- [Helius pricing](https://www.helius.dev/pricing), [credit costs](https://www.helius.dev/docs/billing/credits), [history metering](https://www.helius.dev/docs/rpc/gettransactionsforaddress), and [project credit-cycle usage](https://www.helius.dev/docs/api-reference/admin/get-project-usage).
- [BNB public endpoints](https://docs.bnbchain.org/bnb-smart-chain/developers/json_rpc/json-rpc-endpoint/).
- [PancakeSwap V2 deployments](https://developer.pancakeswap.finance/contracts/v2/addresses) and [Sourcify verification](https://sourcify.dev/server/v2/contract/56/0x16b9a82891338f9bA80E2D6970FddA79D1eb0daE?fields=all).
- [Etherscan supported chains](https://docs.etherscan.io/supported-chains), [Moralis pricing](https://moralis.com/pricing/), [NodeReal pricing](https://nodereal.io/pricing), [address-history contract](https://docs.nodereal.io/reference/nr_gettransactionbyaddress), and [migration guide](https://docs.nodereal.io/docs/migrating-from-bscscan).


## Acceptance

100 Rust tests, the production web build and 26 browser checks passed (15
baseline plus 11 BNB/credit checks) at 320/390/768/1440px. BNB checks use received
public wallet executions; the quota disclosure uses test-only status injection,
not a live credential or invented trade. Long source hashes wrap on mobile.
An active collector retained the previous 11 wallets and 7,290 references across
the schema-compatible upgrade. A subsequent stop/start retained 8,028 references
and continued its same-day counter from 2,138 to 2,245. The initial default daily
2,000 cap paused collection as intended; local continuation used the supported
2,500 test ceiling without resetting counters.

The live WBNB scan received market and chain-56 contract/supply evidence in 0.44s
while collection was active; holder concentration stayed unknown. SOL/RH scans
also received market and direct-chain evidence. The actual BNB test wallet was
manually nominated from a public transaction, then its collector found two
additional actual transfer references. Missing traces/state yielded unresolved
execution, not profit. No live automatic BNB discovery coverage or complete
60-day BNB history is certified. Compact evidence:
[`bnb-and-credit-checks.json`](research/wallet-tracker/bnb-and-credit-checks.json).
