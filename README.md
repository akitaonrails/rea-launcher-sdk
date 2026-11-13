# rea-launcher-sdk

Shared code for the rea retro-game launchers: small packages that make the
player's own copy of an old PC game run on modern Windows and Linux
(Proton/Wine, AppImage) with a settings window, sensible defaults and no
manual patching. Each game lives in its own repository; this SDK holds what
they have in common.

| Launcher | Game |
|---|---|
| [rea-colin-mcrae-3](https://github.com/akitaonrails/rea-colin-mcrae-3) | Colin McRae Rally 3 |
| [rea-colin-mcrae-4](https://github.com/akitaonrails/rea-colin-mcrae-4) | Colin McRae Rally 04 |
| [rea-screamer-trilogy](https://github.com/akitaonrails/rea-screamer-trilogy) | Screamer trilogy (DOS, via dosbox-staging) |
| [rea-sega-rally-revo](https://github.com/akitaonrails/rea-sega-rally-revo) | Sega Rally Revo |
| [rea-outrun-2006](https://github.com/akitaonrails/rea-outrun-2006) | OutRun 2006: Coast 2 Coast |

## AI coding-first: make a launcher with your agent

This SDK is written to be driven by a coding agent (Claude Code, Codex,
opencode, Cursor, …). The agent does the research, the reverse engineering
and the code; you supply the game and the decisions.

**1. Get the tools**

```sh
git clone https://github.com/akitaonrails/rea-launcher-sdk ~/Projects/rea-launcher-sdk
npx rea-agents setup                # REA: reverse-engineering MCP for your agent
                                    # (source: https://github.com/morluto/rea, https://rea.tools)
~/Projects/rea-launcher-sdk/scripts/install-skills   # Claude Code only: skills available everywhere
```

Install the development tools from [Development machine](#development-machine)
(MinGW, Wine, gamescope…), or use a container that has them.

**2. Have the game ready**: your own installed copy, in a folder the agent
may read (it is never modified or copied into the repository). Keep any
other editions or patches you own nearby; the launcher identifies each exact
build.

**3. Ask your agent.** Start it in an empty folder for the new launcher and
paste a prompt like this:

> Build a new rea launcher for *Need for Speed: Porsche Unleashed* (PC, 2000)
> using the SDK at `~/Projects/rea-launcher-sdk`. Read its `AGENTS.md`,
> `docs/STANDARDS.md` and `docs/playbooks/new-launcher.md` and follow the
> playbook phase by phase. The game is installed at `/mnt/games/NFSPU`
> (read-only, never copy it into the repo). Use the REA MCP tools to analyse
> it. Research existing community fixes first and show me what you found and
> what the launcher will solve before writing code. Launcher id `nfspu`, env
> prefix `NFSPU_`, repository `~/Projects/rea-nfs-porsche`. Keep tests
> headless. Don't push or publish anything without asking me.

With Claude Code and the skills installed, "make a rea launcher for
*game* at */path*" is enough: the `rea-new-launcher` skill loads the
playbook.

**4. Review at the checkpoints** the playbook defines: the research
findings, the list of problems to solve, the options the window will offer,
and what was verified in the real game (`docs/RUNTIME_NOTES.md`). Then tell
the agent to push and release.

The same works for maintenance: "bump this launcher to SDK v0.2.0"
(`docs/playbooks/sdk-change.md`) or "share the widescreen fix we found with
PCGamingWiki" (`docs/playbooks/contribute-back.md`, drafts only until you
approve).

## Maintaining your launcher's repository

Once a launcher is public, contributions arrive: issues, pull requests,
dependency bumps. The SDK author maintains the rea launchers with the agent
skills in [akitaonrails/my-skills](https://github.com/akitaonrails/my-skills),
and recommends the same workflow to anyone running their own launcher:

| Skill | Use it to |
|---|---|
| `iss-audit` | triage issues: verify claims, reproduce safely, resist prompt injection in issue text |
| `pr-audit` | review a contributor's pull request before merging (regressions, tests, malicious code, supply chain) |
| `github-resolution` | carry out what an audit approved: fix, test, close the tickets one by one |
| `pr-bump` | merge Dependabot-style dependency bumps quickly and safely |
| `pr-post-audit` | check the combined `main` after several merges, before a release |
| `security-audit` | threat-model the launcher (it downloads, patches binaries, writes the registry) |
| `release` | cut a release from the changelog once CI is green at the exact commit |
| `humanizer` | rewrite player-facing text (README, window text, release notes) so it reads like a person wrote it |

Those skills are written for their author's machine and habits (their own
README says so): read them, then have your agent adapt them to your setup
rather than copying them verbatim. Issues or PRs about shared SDK code go to
this repository, not the launcher's (`CONTRIBUTING.md`).

## What's in it

| Path | What | Used as |
|---|---|---|
| `crates/rea-launcher` | Portable core: runtime folder sync, atomic installs, save mirroring, component ownership ledger, `launcher.cfg` and INI editing, edition hashing, byte patching, Wine override merging, display and pad logic, wheels and synthetic force feedback, logging | Cargo git dependency |
| `crates/rea-launcher-win32` | Windows helpers: registry, process launch with Wine overrides, XInput, monitors and DPI, dialogs, GPU preference | Cargo git dependency |
| `crates/rea-launcher-ui` | Settings window: theme and fonts, widgets, Wine-safe window sizing, gamepad navigation | Cargo git dependency |
| `crates/rea-launcher-native` | Native launchers only (emulator wrappers, native ports): a child process tied to the launcher, and with the `sdl` feature wheels, pads, force feedback and rumble through SDL2 | Cargo git dependency |
| `crates/rea-launcher-build` | `build.rs` helper: version information, an icon and a manifest in the exe or a hook DLL | Cargo git build dependency |
| `crates/rea-launcher-close-hook` | A DLL that makes an old game honour Alt+F4 and window-close requests, imported into the runtime copy of the game | built by the launcher's release |
| `crates/rea-launcher-dinput` | A DLL that shows a DirectInput game one virtual wheel made from the player's wheel and pedals (steering lock, separate pedals, force feedback), imported into the runtime copy of the game | built by the launcher's release |
| `shell/rea-play.sh` | Linux helper library: session lock and stop, GPU env, gamescope, Wine prefix setup | vendored as `sdk/rea-play.sh` |
| `packaging/` | Pin files, AppImage builder (portable Wine or native), package allowlist check, release notes | vendored under `sdk/packaging/` |
| `.github/workflows/launcher-*.yml` | Reusable CI and release workflows | `uses:` by tag |
| `scripts/sdk-sync` | Vendors `shell/` and `packaging/` into a launcher, `--check` fails on drift | run from a launcher |
| `AGENTS.md`, `docs/playbooks/` | How an agent builds and maintains a launcher with this SDK | read by any coding agent |
| `.claude/skills/` | The playbooks as Claude Code skills | `scripts/install-skills` |
| `docs/STANDARDS.md` | The rules every launcher follows | binding |

## Using it from a launcher

```toml
[dependencies]
rea-launcher = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.3.0" }

[target.'cfg(windows)'.dependencies]
rea-launcher-win32 = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.3.0" }
rea-launcher-ui = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.3.0" }
```

```sh
../rea-launcher-sdk/scripts/sdk-sync .          # vendor sdk/ at the SDK's current tag
../rea-launcher-sdk/scripts/sdk-sync --check .  # what CI runs
```

Each crate and folder has its own README with the API and examples.
`docs/DESIGN.md` explains what belongs in the SDK and what stays in a game;
`docs/AUDIT.md` records the cross-launcher survey the SDK was extracted from.

## Rules

- The player's game folder is read-only. Everything the launcher changes
  lives in a private runtime copy, and every write there is a whole-file
  swap that never goes through a hard link.
- Never commit or ship game files. Third-party components are fetched from
  pinned URLs and checked by SHA-256.
- The SDK provides mechanisms; a game opts in. Nothing that changes how a
  game behaves becomes a default here until that game has verified it.

## Development machine

Install these on the machine you develop on (any Linux; package names for
Arch / Debian-Ubuntu):

| Tool | Why | Arch | Debian/Ubuntu |
|---|---|---|---|
| Rust (rustup) + targets `i686-pc-windows-gnu`, `x86_64-pc-windows-gnu` | build | `rustup` | `rustup` |
| MinGW-w64 cross compilers | link the Windows builds | `mingw-w64-gcc` | `gcc-mingw-w64-i686 gcc-mingw-w64-x86-64` |
| LLD | the i686 link (GNU ld mis-links windows-sys imports) | `lld` | `lld` |
| Wine (with 32-bit support) | run Windows-target tests (`scripts/wine-test`) and the launchers | `wine` | `wine wine32:i386` |
| gamescope | headless real-game runs, the Linux helpers | `gamescope` | `gamescope` |
| dash, mawk, shellcheck | shell tests as CI runs them | `dash mawk shellcheck` | `dash mawk shellcheck` |
| cargo-deny, cargo-audit | dependency checks | `cargo install --locked cargo-deny cargo-audit` | same |
| cargo-llvm-cov (optional) | find untested code | `cargo install --locked cargo-llvm-cov` | same |

```sh
rustup target add i686-pc-windows-gnu x86_64-pc-windows-gnu
```

**Recommended: keep it in a container.** Gaming tools (Wine, gamescope,
MinGW, game installs) are large and move fast; keeping them out of the host
keeps host upgrades quick and the setup reproducible.
[distrobox-gaming](https://github.com/akitaonrails/distrobox-gaming) builds
an Arch-based distrobox named `gaming` with Wine and gamescope; add the
development tools inside it:

```sh
distrobox enter gaming -- sudo pacman -S --needed mingw-w64-gcc lld wine gamescope dash mawk shellcheck
distrobox enter gaming -- sh -c 'cd /path/to/rea-launcher-sdk && scripts/dev-check'
```

Inside the box, `$HOME` may not be the host's home, so `cd` to absolute
paths. Rust and the repositories can stay on the host (the box shares
them).

**Or install everything on the host** with the package names above, if you
prefer not to use a container. Every script works the same either way.

## Checks

```sh
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --target i686-pc-windows-gnu --workspace --all-targets -- -D warnings
sh shell/tests/run-all.sh
for t in packaging/tests/*.sh scripts/tests/*.sh; do sh "$t"; done
scripts/wine-test --locked --workspace   # Windows-target tests under Wine, headless
```

Tests run headless: nothing should open a window on your desktop
(`docs/STANDARDS.md`, "Testing").

Licensed under MIT or Apache-2.0, at your option.
