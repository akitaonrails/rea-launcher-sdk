---
name: rea-new-launcher
description: Build a new rea launcher that makes an old PC game plug-and-play on modern Windows and Linux with rea-launcher-sdk — community-fix research first, REA reverse engineering of the game folder, scaffold, implement, headless verification, release. Use when asked to create, start, or scaffold a launcher, compatibility package, or "rea-<game>" project for a game, or to make an abandoned/old PC game run on modern systems.
---

# New rea launcher

Follow the SDK's playbook exactly; it is the source of truth:

1. Locate the SDK checkout (ask if unknown; default `~/Projects/rea-launcher-sdk`
   or clone `https://github.com/akitaonrails/rea-launcher-sdk`).
2. Read `<sdk>/docs/STANDARDS.md` (binding) and
   `<sdk>/docs/playbooks/new-launcher.md`, then work through its phases in order.
3. Make sure the REA MCP tools are available (`npx rea-agents setup` registers
   them); use them for Phase 2 on the game folder, read in place.

Non-negotiable, even if the user doesn't repeat it:
- Research existing community fixes online before writing any fix; record them
  in `docs/RESEARCH.md` and build on them.
- Never commit or copy game files; the game folder is read-only.
- Everything outside the new repository is the user's and read-only (other repos,
  prefixes, saves, configs) unless they ask or approve that change; back up first.
- Tests, probes and game runs are headless; no windows pop up on the user's desktop.
  Open one only when a check needs a person or headless would prove nothing, and
  tell the user first what will open and why.
- Settings-window work goes through a design skill; accessibility first; every
  option and CLI flag gets an end-to-end test; no confusing options.
- Push, tag, or post anything public only with the user's go-ahead.
- Run `sdk/scripts/launcher-check` before every commit and before a release.
  Its errors are blocking: fix them (`--explain RULE` says how). Never edit
  `.rea-check-waivers`, turn off `conformance` or reword a file just to
  silence a rule; a waiver is the owner's decision, with its reason.
- Every fix of our own gets its draft in `docs/CONTRIBUTIONS.md` in the
  commit that ships it.
- A build with DRM or a disc check removed (pre-activated, no-CD) is
  supported only after the owner decides; ask, then follow STANDARDS
  "DRM exceptions".
