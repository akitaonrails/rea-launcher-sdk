# rea-launcher-sdk — instructions for coding agents

This repository is the shared SDK of the rea launchers: packages that make a
player's own copy of an old PC game run on modern Windows and Linux. Read
`docs/STANDARDS.md` before any work; it is binding for the SDK and for
every launcher built on it.

## What are you here to do?

| Task | Read |
|---|---|
| Build a launcher for a new game | `docs/playbooks/new-launcher.md` |
| Change the SDK, or bump a launcher to a new SDK version | `docs/playbooks/sdk-change.md` |
| Set up GitHub Actions to build and publish release binaries | `docs/playbooks/ci-and-releases.md` |
| Pin, bundle, download and configure community fixes and packs | `docs/playbooks/community-components.md` |
| Reverse engineer, test and apply a binary patch | `docs/playbooks/binary-patching.md` |
| Share a fix we found with PCGamingWiki / upstream | `docs/playbooks/contribute-back.md` |
| Understand what lives here and why | `docs/DESIGN.md`, `docs/AUDIT.md`, each crate's README |

## Rules that are easy to forget

- **Research community fixes first** (PCGamingWiki, GitHub, mod sites)
  before reverse engineering anything. Record them in the launcher's
  `docs/RESEARCH.md`.
- **The game folder is read-only, and nothing from it is ever committed.**
- **Everything outside the repository you work in is read-only** (game
  installs, other repos, prefixes, saves, home configs, containers) unless the
  user asked for or approved that specific change. Back up first; never
  delete by a broad glob; when unsure, ask.
- **Headless by default.** Tests and probes never open windows on the
  owner's desktop: `scripts/wine-test` for Windows-target tests,
  `gamescope --backend headless` for game runs. Open a window only on
  purpose and say so.
- **Settings window changes go through a design skill**; accessibility
  first; every window option and CLI flag has an end-to-end test.
- **Credentials** (e.g. `~/.config/rea/pcgamingwiki.txt`) are never printed,
  logged, committed, passed as command-line arguments or handed to other
  agents.
- **Public actions need the owner's go-ahead**: pushing, tagging releases,
  posting issues, wiki edits.
- **Clean up after releases and long sessions** with `scripts/clean`
  (preview, then `--yes`): build output and task scratch only; keep evidence
  and anything that isn't yours.
- **Windows code must be run, not only linted**: link the i686 build and run
  `scripts/wine-test`.

## Where the tools are

MinGW, lld, Wine and gamescope may live on the host or in a container
(the recommended setup is the `gaming` distrobox from
github.com/akitaonrails/distrobox-gaming). If a tool is missing on the
host, check `distrobox list` for `gaming` and run the command there:
`distrobox enter gaming -- sh -c 'cd /absolute/path && …'` — always an
absolute path, since the box's `$HOME` can differ. When the box's view of
`/home` is read-only (seen after a box restart: cargo can't update its git
cache, or finds only the system toolchain), point the toolchain at the
box's own home: `H=<box home>; HOME=$H CARGO_HOME=$H/.cargo
RUSTUP_HOME=$H/.rustup PATH=$H/.cargo/bin:$PATH` (`distrobox enter gaming --
sh -c 'echo $HOME'` shows it). If neither has it, tell
the user what to install (README, "Development machine") rather than
skipping the check silently.

## Working on the SDK itself

- `scripts/dev-check` runs every check (it skips tools that are missing,
  with a warning). Everything must pass before a commit.
- Pure logic goes in `crates/rea-launcher` with host tests; `unsafe`/FFI only
  in `crates/rea-launcher-win32`; egui in `crates/rea-launcher-ui`.
- A change that alters a launcher's behaviour is opt-in, or listed in the
  crate README's MIGRATION section and the CHANGELOG.
- Conventional Commits. Issues and PRs about shared code belong here, not in
  the launcher repositories (`CONTRIBUTING.md`).
