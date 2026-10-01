# Launchpad marks

The Pump.fun and StonkFun marks in this folder are vendored from
[metasal1/solana-icons](https://github.com/metasal1/solana-icons), which is
published under the MIT License.

They are used only to identify the launchpad Water has matched from onchain
evidence. Brand names and trademarks remain the property of their respective
owners.

## Robinhood Chain marks

Retrieved 2026-10-01 from each project's own website. These are identification
assets, not endorsements or evidence of launch provenance. Recognition still
comes exclusively from the backend's chain/provider evidence.

No open redistribution license was stated for these first-party marks; their
copyright and trademarks remain with their owners. They are not covered by the
Solana icon repository's MIT license.

Raster marks are resized proportionally to at most 128px for the 34px UI slot;
SVG marks retain their original geometry.

The runtime mapping and per-asset provenance are in
`web/lib/launchpad-marks.json`. All rendered images are local.

| Slug | Local file | First-party source |
| --- | --- | --- |
| `pons` | `pons.png` | https://www.ponsfamily.com/pons.png |
| `hoodfun` | `hoodfun.png` | https://hood.fun/hood-mark.png |
| `noxa` | `noxa.png` | https://fun.noxa.eth.limo/favicon.png |
| `stonkbrokers` | `stonkbrokers.png` | https://www.stonkbrokers.cash/icon-192.png |
| `hookr` | `hookr.svg` | https://hookr.fun/icon.svg |
| `froth` | `froth.png` | https://froth.meme/favicon/apple-touch-icon.png |
| `pyre` | `pyre.svg` | https://pyre.fun/favicon.svg |
| `pairyard` | `pairyard.svg` | https://pairyard.com/icon.svg |
| `merryforge` | `merryforge.svg` | https://www.merryforge.app/merryforge-logo.svg |
| `raisehood` | `raisehood.svg` | https://www.raisehood.xyz/favicon.svg |
| `perpshood` | `perpshood.png` | https://perpshood.fun/assets/perps-hood-logo-DVlKzCf0.png |
| `ponzu` | `ponzu.svg` | https://ponzu.app/lemon-logo-big-face.svg |
| `v4fun` | `v4fun.png` | https://v4.fun/icon.png |
| `peeps` | `peeps.webp` | https://peeps.wtf/logo-launchpad.webp |

### Intentionally unresolved

Keep the existing initial fallback for these slugs until a trustworthy mark can
be retrieved and verified. Do not substitute a chain logo, token art from an
unrelated project, or a guessed favicon.

| Slug | Reason |
| --- | --- |
| `longxyz` | long.xyz returned HTTP 403; www.long.xyz returned HTTP 404. |
| `coinbarrel` | coinbarrel.com and docs.coinbarrel.com returned HTTP 403. |
| `tokenselect` | token.select returned HTTP 403; Select Foundation docs did not identify a usable product mark. |
| `arrowpad` | arrowpad.fun and docs.arrowpad.fun returned HTTP 403. |
| `parfamily` | par.family and its docs returned HTTP 403. |
| `unihood` | unihood.fun and its docs returned HTTP 403. |
| `pairex` | pairex.market and its docs returned HTTP 404. |
| `robinpad` | robinpad.fun and robinpad.app present different brands; the current creator-label match cannot distinguish them. |

NOXA uses the original launchpad's `fun.noxa.eth.limo` favicon, not the newer
similarly named site whose relationship to the original project is unverified.

The image component falls back to initials on a local asset load failure and
reserves its 34px dimensions to avoid shifting the launchpad identity.
