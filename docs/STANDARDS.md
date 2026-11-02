# Launcher standards

The rules every rea launcher follows. Each launcher's `AGENTS.md` repeats the
non-negotiables in its own words and adds what is specific to its game.

## Non-negotiables

1. **Never ship the game.** Never commit, vendor, embed or publish original
   executables, DLLs, media, archives, fonts, tracks, car data or any other
   game asset, or anything copied out of them. Tests use tiny synthetic
   fixtures, never bytes taken from the game.
2. **The player's game folder is read-only.** The launcher builds a private
   runtime folder from it and changes only that copy. Every write there is a
   whole-file swap (`rea_launcher::fsx`), never a write through a hard link,
   and the runtime may never overlap the game folder
   (`rea_launcher::paths::check_separate`).
3. **Never commit a developer's absolute game path.** Code and tests take an
   explicit path or a documented `<GAME>_GAME_DIR`-style variable.
4. **Local investigation goes in `.work/`** (ignored by Git): REA snapshots,
   traces, test prefixes.
5. **Third-party components are pinned.** Fetched at package time from URLs
   with SHA-256 (`packaging/*.pins`), never committed. Every component is
   listed with its licence and source in `THIRD_PARTY.md`; check the
   licence before bundling. A file with no licence is never bundled — the
   launcher downloads it on the player's request instead (OutRun's HD pack).
6. **Exact binary identity before patching.** Executables are identified by
   SHA-256 (`rea_launcher::identify`). Unknown builds are refused with their
   digest, never patched by guesswork. Byte patches check the original bytes
   (or the already-patched bytes) at every site and are all-or-nothing
   (`rea_launcher::patch`).

## Research before building

Before writing any fix for a new game, search online for what the community
already made, and build on it. Every launcher so far stands on someone
else's work:

| Launcher | Community work it builds on |
|---|---|
| CMR3 | SilentPatch (widescreen, fixes, ASI loader), Bekoha's HD UI, the official 1.1 patch |
| CMR4 | community widescreen patch signature, dgVoodoo2, DSOAL users' setups |
| OutRun 2006 | OutRun2006Tweaks (fixes, input, framerate), the Soundtrack Fix, the community HD texture pack |
| Sega Rally Revo | the community frame-pacing fix |
| Screamer | dosbox-staging, GOG's layouts |
| all Wine games | DXVK, Xidi |

Where to look: PCGamingWiki (the game's page: fixes, widescreen, controller,
known issues), the Widescreen Fixes Pack / ThirteenAG and Silent's
projects, GitHub searches for the game's name plus "fix", "patch",
"widescreen", "tweaks", ModDB/Nexus, Steam and GOG forums, speedrunning and
fan communities, ProtonDB and the WineHQ AppDB for Linux quirks.

Record what you find in `docs/RESEARCH.md`: each project, what it fixes,
its licence, whether it is maintained, and the decision (bundle, configure,
download on request, or not used and why). Only reverse-engineer and patch
ourselves what nobody has solved; credit everything in `THIRD_PARTY.md` and
the README.

## Contribute back

When our own reverse engineering finds a fix the community doesn't have (a
widescreen or frame-rate patch, a crash cause, a Wine/Proton setting that
makes the game work), share it where players look:

- **PCGamingWiki**: the game's page (fix boxes, widescreen, controller,
  Linux sections). Describe the fix as edits a player can make — offsets
  with original and new bytes, INI keys, registry values — plus the exact
  executable it applies to (SHA-256, version). Cite our repository. Never
  attach or link game files.
- **Upstream projects** we build on (SilentPatch, OutRun2006Tweaks,
  dosbox-staging, DXVK, Xidi): an issue or pull request when the fix
  belongs in their code.
- **ProtonDB / WineHQ AppDB**: a report with the settings that work.

These are public posts under the owner's name, so the agent only drafts
them: write each draft in `docs/CONTRIBUTIONS.md` (target page or project,
text ready to paste, evidence, status) and tell the owner. The owner
reviews and posts. Only publish fixes verified in the real game.

## Product rules

- **Self-contained.** The player installs the package and points it at the
  game. No third-party patchers, winetricks commands typed by hand, DLL
  overrides, compatibility modes, codec packs or registry edits.
- **Windows first, easy on Linux.** A Windows build that also runs under
  Proton or Wine with nothing extra, plus an AppImage with a portable Wine
  (or a native Linux build when the game runs natively, like Screamer).
- **Reuse open source.** When a maintained project already fixes something
  (SilentPatch, OutRun2006Tweaks, Xidi, DXVK, dgVoodoo2, dosbox-staging),
  bundle and configure it. Write our own patch only when nothing exists.
- **The modern experience is the default; the original stays one click
  away.** Offer an "Original" look that runs the game as shipped, a
  recommended look picked from the player's screen, and a Steam Deck look.
- **The game's own settings stay the game's.** Write only the keys the
  launcher owns; keep comments, unknown keys and the player's choices. Unset
  in `launcher.cfg` means "keep the game's value".
- **Saves are sacred.** Never delete a save. Mirror saves with recorded
  state (`rea_launcher::saves`): a save the player changed wins, the
  runtime wins only when it alone changed.
- **No unverified options.** An option reaches the settings window only
  after its effect has been seen in the real game. Record what was checked
  and what was not in `docs/RUNTIME_NOTES.md`.

## Code

- Rust for our code; edition 2024. `unsafe` and Win32 FFI only inside
  `rea-launcher-win32` or one narrow `win32.rs` module.
- General solutions first: find the one place a whole class of cases goes
  through and fix it there.
- Use the SDK for everything it covers. Never copy SDK code into a launcher;
  if it lacks something, keep the local code and propose the change upstream.
- Host-testable logic gets unit tests before its milestone commit: edition
  detection, patch tables, runtime planning, config and INI editing, form
  state. Shell helpers get tests in `linux/tests/`.
- Conventional Commit subjects. Commit at working milestones, only after the
  full check passes (fmt, tests, shell tests, Windows-target clippy, deny).
- Every behaviour change gets a CHANGELOG "Unreleased" entry; every change
  that still needs a real-game check gets a line in `docs/RUNTIME_NOTES.md`.

## Settings window and CLI

The player meets the launcher through its window and its command line, so
these rules are firm:

- **Design with a design skill.** Any change to the settings window's look
  or layout goes through a design skill (`frontend-design`, then a review
  against `web-design-guidelines`), working within the SDK theme
  (`rea-launcher-ui`). Don't hand-tweak colours, spacing or wording
  without it.
- **Accessibility comes first.**
  - Text and controls meet WCAG AA contrast; each launcher keeps a test of
    its palette (`Palette::contrast_problems`).
  - Everything works with mouse, keyboard and gamepad alone, with a
    visible focus ring.
  - Readable text sizes at the window's default zoom; nothing conveyed by
    colour alone.
  - A disabled control says why.
- **No confusing options.**
  - Every option has a plain-language label and a one-line description of
    what the player will see change.
  - An option that has no visible effect, overlaps another, or needs
    technical knowledge to choose is removed or folded into a look.
  - Defaults are the recommended experience; the player should be able to
    press Play without touching anything.
- **Every option is tested end to end.** For each window option and CLI
  flag, a test proves the wiring: control → `launcher.cfg` key → the game
  file, registry value or environment it changes, and the flag → the same
  key. Window behaviour is tested headless with `egui_kittest` (find
  controls by label, click, check the saved settings). CLI tests cover
  `--help`, every flag, bad values and their messages.

## Testing (soft rules)

- **Headless by default.** Automated tests and probes should not open
  windows on the developer's desktop. Clear `DISPLAY`/`WAYLAND_DISPLAY` for
  Wine runs, switch Wine's crash dialog off in a private test prefix
  (`scripts/wine-test` does both), and run the real game in a headless
  compositor (`gamescope --backend headless`) with a PipeWire null sink.
  When a check really needs a visible window — judging the settings
  window's look, a controller test — open one deliberately and say so; just
  don't make it the default.
- **Exercise Windows code, not only lint it.** Link the i686 build and run
  the Windows-target tests under Wine with `scripts/wine-test`; clippy alone
  never runs `cfg(windows)` code.
- **The real game stays outside the repository** and outside ordinary host
  tests. Env-gated tests (`<GAME>_GAME_DIR=... cargo test -- --ignored`)
  may use it.

## Repository shape

```
AGENTS.md  CHANGELOG.md  README.md  THIRD_PARTY.md  LICENSE-MIT  LICENSE-APACHE
Cargo.toml  Cargo.lock  deny.toml  .cargo/config.toml  .gitignore
crates/<id>-launcher/      src/{main,config,form,ui,edition,runtime,...}.rs
linux/<id>-play            sources sdk/rea-play.sh
linux/tests/*.sh
packaging/                 pins, allowlist, desktop entry, icon, package script
sdk/                       vendored by scripts/sdk-sync (do not edit)
docs/RUNTIME_NOTES.md      what was verified in the real game, and what wasn't
.github/workflows/build.yml  calls the SDK's launcher-ci / launcher-release
```
