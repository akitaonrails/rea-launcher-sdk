---
name: rea-sdk-change
description: Change rea-launcher-sdk or move a rea launcher to a new SDK version — fill an SDK gap found in a launcher, release a new SDK tag, bump launchers (Cargo tag, sdk-sync, reusable workflows), or audit launchers for drift and duplicated code. Use when a rea launcher needs something the SDK lacks, when asked to bump/upgrade the SDK in a launcher, or to compare the launchers.
---

# Change the SDK / bump launchers

Read `<sdk>/docs/STANDARDS.md` and follow `<sdk>/docs/playbooks/sdk-change.md`.
Never copy SDK code into a launcher or edit a launcher's vendored `sdk/`. Run
`<sdk>/scripts/dev-check` (SDK) or the launcher's full check plus
`scripts/wine-test` before each commit. Tests stay headless.

In a launcher, `sdk/scripts/launcher-check` runs before every commit and
before a release, and its errors block both; after a bump, the new
version's rules apply. Waivers (`.rea-check-waivers`) are the owner's
decisions only. In the SDK, a mistake a launcher repeated that a file can
show becomes a `scripts/launcher-check` rule with a test
(sdk-change.md, "When a launcher repeats a mistake").
