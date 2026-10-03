# Robinhood launchpad evidence

Verified 2026-10-01. `core/src/robinhood_launchpads.rs` checks chain ID 4663,
then calls `getLaunchedToken(address)` (selector `0x3cf28b5a`) on a bounded
registry of first-party factory addresses. A match requires the exact ABI length,
the requested token in the first word, `exists == true`, and a valid launch
creator. A token's self-reported factory/name/suffix is never sufficient.

| Launchpad | Factory | ABI | First-party source |
| --- | --- | --- | --- |
| Pons v2 | `0x7eD598BcEf8bd9Edd8C97A195C6d13f40801EC7e` | 15 static words; exists at 14; creator at 2 | https://docs.ponsfamily.com/v2 |
| Pons v1 | `0xA5aAb3F0c6EeadF30Ef1D3Eb997108E976351feB` | 13 static words; exists at 11; creator at 1 | https://docs.ponsfamily.com/ |
| Pons legacy v1 | `0x0c37a24F5D23A486FA692d1500881d698B1F77a4` | Same v1 ABI | https://docs.ponsfamily.com/ |
| NOXA Fun | `0xD9eC2db5f3D1b236843925949fe5bd8a3836FCcB` | Same v1 ABI | https://fun.noxa.eth.limo/assets/index-DsXVcCyP.js linked by https://fun.noxa.eth.limo/ |

The NOXA frontend's `rh` network config declares chain 4663 and the factory above;
its ABI declares all 13 fields, including the `exists` flag. Pons docs publish both
the addresses and full return layouts. The Pons v1 reference token is
`0x39dBED3a2bd333467115dE45665cC57F813C4571`, launch transaction
`0x1f54f25fec2d963dcb338ecb8b46a6eb123198a5c7a746d34cb2dbe78d074af8`.

The registry does not identify generic Uniswap pools or Doppler infrastructure
as a particular frontend. Shared infrastructure alone cannot prove Long.xyz or
another frontend originated a launch. Other previously recognized launchpads
retain the explicit explorer-label evidence path; arbitrary substrings no longer
match (e.g. `ResponseFactory` must not become Pons).

Factory checks run independently of Blockscout and do not require an API key.
A failed optional creator lookup does not discard factory evidence. The onchain
launch creator and the explorer's contract creator have distinct meanings; Water
preserves the indexed creator when available and uses the label `Launch creator`
when only the factory record supplies the person who launched the token.

Adding a factory requires a first-party source, its precise return schema, a
positive real-token check, and negative tests. Unsupported or unverifiable
launches remain unknown.

Live validation samples (2026-10-01):

- Pons legacy v1: `0x39dBED3a2bd333467115dE45665cC57F813C4571`.
- Pons v1: `0x055650555be80649397084cd3f8a09b4350e8612`.
- Pons v2: `0x376981c2c9c36545e06f6538979c7844836f7755`.
- NOXA Fun: `0x6399e2bd8af62c0ac13f55613c3469b67332a6fd`, discovered from the factory’s `TokenLaunched` event and then independently verified through the origin endpoint.
- Negative control: RH WETH `0x0Bd7D308f8E1639FAb988df18A8011f41EAcAD73` remains unrecognized.

All four positive samples passed the candidate origin endpoint without an explorer API key. Validation run: https://github.com/pseudodev3/Water/actions/runs/36852543612.

## Long.xyz LongLauncher (2026-10-03)

Exact-match deployed source is the first-party contract evidence:
https://sourcify.dev/server/v2/contract/4663/0x22e99278308b393ea1260859b181ad7e78f5eeed?fields=all
(`src/LongLauncher.sol:LongLauncher`, creation and runtime both `exact_match`).
The source begins `Copyright (c) 2026 long.xyz. All rights reserved.` Deployment
transaction `0x717af93c071b39247b5cec72930b990b917439143f6621d08797090732947cf9`
is at block 8,636,038. The source is referenced, not copied into Water.

Recognition requires the `LaunchCreated` event from that exact launcher,
with the requested token in indexed `asset` (topic 2). Event signature:
`LaunchCreated(address,address,address,address,address,bytes32,uint48,uint48,string)`;
topic 0 is `0xadc6f1f726f7c710f77ec06adc75f3bb964e5be19581b072c67f7b9b4039267b`.
The ABI has four topics; data contains poolInitializer, launcher, tickerKey,
deployedAt, reservedUntil and a dynamic normalizedTicker (1–15 uppercase ASCII
letters). The string offset is 192 bytes; total data is 256 bytes. The source
requires reservedUntil = deployedAt + 86,400. The event's launcher is the launch
creator, kept distinct from an indexed contract creator.

Only non-removed logs with canonical address padding, a valid transaction/block
hash and a block inside the requested history are accepted. All queried ranges
must succeed, and exactly one launch record must match. Duplicate/conflicting
records, malformed logs, partial history, wrong chain and timeouts stay unknown.
The public RPC allows at most 10 million blocks per query. Water searches from
deployment to a captured head in non-overlapping parallel ranges, capped at 16
requests and a 4.5s total Long lookup budget. Beyond this coverage cap recognition
stays unknown until the lookup strategy is expanded. Existing factory checks run
independently and survive a failed or slow Long lookup.

This proves use of the Long.xyz-authored launcher. It does not prove which web
frontend submitted a transaction; the launcher is publicly callable. Shared
Doppler Airlock / hooks, trading routers, names and address suffixes never count
as Long evidence. Newer launcher addresses are outside this verified registry.
`app.long.xyz` currently returns HTTP 403 to this environment, so no unverified
frontend configuration or brand asset was added.

Positive reference: VIBE `0x782a6a4653896be2ff3fd885d50895d7e19a1e18`,
transaction `0x76b87af28d7faacec8b645824fe337d4cf0a61e2d958fc39909f6dd41f1782e6`,
block 51,337,219, launch creator `0xa99c430efbdf4d2d4110d32978a2eceee21ce79a`.
Its raw RPC event is retained in `core/tests/fixtures/long-launch-created.json`.
