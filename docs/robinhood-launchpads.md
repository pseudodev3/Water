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
