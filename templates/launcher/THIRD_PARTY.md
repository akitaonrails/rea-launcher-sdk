# Third-party components and assets

Nothing from @GAME@ itself is part of this repository or its packages.
Every component below is fetched from a pinned URL and checked by SHA-256
(`packaging/third-party.txt`), or is an asset with its licence recorded.

| Component | Version | What it does here | Licence | Source | Bundled? |
|---|---|---|---|---|---|
| [rea-launcher-sdk](@SDK_GIT@) | @SDK_TAG@ | Shared launcher code, Linux helper, packaging | MIT OR Apache-2.0 | @SDK_GIT@ | compiled in, `sdk/` |
| Barlow fonts | from the SDK | The settings window's type | SIL OFL 1.1 (`licenses/Barlow-OFL.txt`) | https://github.com/jpt/barlow | compiled in |
| Wine | AppImage only (SDK `packaging/appimage/tools.pins`) | Runs the launcher in the AppImage | LGPL-2.1-or-later | SDK `packaging/appimage/wine/SOURCE.txt` | AppImage |

## Images

| File | Author | Licence | Source |
|---|---|---|---|
| (the window's hero image, once researched) | | | |

## Owner decisions

Record here any decision to bundle a file without a licence, use official
art, or rely on a DRM-removing fix.
