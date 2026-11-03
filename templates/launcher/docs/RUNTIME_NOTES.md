# Runtime notes: @GAME@

What was verified in the real game, how, and what was not. An option
reaches the settings window only after its effect has been seen here
(rea-launcher-sdk `docs/STANDARDS.md`, "No unverified options").

Runs are headless unless a line says otherwise: `gamescope --backend
headless` with a PipeWire null sink, a throwaway Wine prefix under `.work/`.

## Verification matrix

Status: ✅ verified · ⚠️ partly (see notes) · ❌ fails · — not yet checked.

### Editions

| Edition (SHA-256 prefix) | `--check` | Starts | Plays a session | Notes |
|---|---|---|---|---|
| | — | — | — | |
| unknown file | — (refused with SHA-256 and issue link) | — | — | |

### Options and looks

| Option / look | Window → `launcher.cfg` → game file | Effect seen in game | Notes |
|---|---|---|---|
| Look: Original | — | — | |
| Look: Recommended | — | — | |
| Look: Steam Deck | — | — | device check pending |
| `resolution` (example) | — | — | |
| `vsync` (example) | — | — | |

### Platforms

| Platform | Status | Notes |
|---|---|---|
| Windows 11 | — | |
| Windows 10 | — | |
| Proton, without our Linux helper (Steam, non-Steam game) | — | |
| Linux AppImage (portable Wine) | — | |
| Linux, `linux/@ID@-play` with system Wine | — | |
| Steam Deck, Game Mode (controller, 1280×800, suspend/resume) | — | |
| Multi-monitor | — | |
| HiDPI / 4K | — | |
| Ultrawide | — | |

### Quitting and leftovers

Every way of quitting, in the real game, then the process table checked by
name and by command line: no game, launcher, `wineserver` or gamescope left
within a few seconds (`sdk/docs/STANDARDS.md`, "Clean exit, single
instance").

| Quit path | Status | Leftover processes | Notes |
|---|---|---|---|
| Quit from the game's menu | — | — | |
| Window close button / Alt+F4 | — | — | |
| `@ID@-play stop` (SIGTERM) | — | — | |
| Ctrl+C on `@ID@-play` (SIGINT) | — | — | |
| Steam "Stop" (Proton) | — | — | |
| gamescope closed | — | — | |
| Second launcher start while the game runs | — | — | brings the window forward or says the game is running (exit 5) |

### Robustness

| Check | Status | Notes |
|---|---|---|
| Hostile paths (spaces, apostrophes, accents) for install, game and runtime | — | |
| `launcher.cfg` saved in a Windows code page | — | |
| Upgrade from an earlier release's files | — (no earlier release) | |
| Gamepad-only use of the window | — | |

## Log

Dated entries: what ran, on what, what was seen.
