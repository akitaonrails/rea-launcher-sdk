# Playbook: change the SDK, bump the launchers

## When a launcher needs something the SDK lacks

1. Don't copy SDK code into the launcher and don't edit its vendored `sdk/`.
   Keep the launcher's local code for now and open an issue (or PR) in
   `akitaonrails/rea-launcher-sdk` describing the gap: which launcher, what
   it needs, file:line.
2. In the SDK, make the change generic: game knowledge comes in as data or a
   trait implementation, a new behaviour is opt-in until every launcher has
   verified it. Add tests (host and `scripts/wine-test`), update the crate
   README (and its MIGRATION notes if behaviour differs), `CHANGELOG.md`.
3. Run `scripts/dev-check`. Release: bump the workspace version, CHANGELOG
   section, tag `vX.Y.Z`, push with the tag.

## Bumping a launcher to a new SDK version

1. Change the `tag = "vX.Y.Z"` of every `rea-launcher*` dependency in its
   `Cargo.toml`; `cargo update -p rea-launcher -p rea-launcher-win32
   -p rea-launcher-ui`.
2. `<sdk>/scripts/sdk-sync <launcher>` to re-vendor `sdk/`; update the
   `@vX.Y.Z` of the reusable workflows in `.github/workflows/`.
3. Read the SDK CHANGELOG for the versions skipped; adapt and delete any
   local workaround the new version makes unnecessary.
4. Run the launcher's full check, `scripts/wine-test`, `sdk-sync --check`;
   CHANGELOG "Unreleased" entry; commit `build: bump rea-launcher-sdk to
   vX.Y.Z`.

## Auditing the launchers for drift

Every few releases, compare the launchers feature by feature (gamepad,
settings window, config, runtime, Linux helper, packaging, CI) the way
`docs/AUDIT.md` did: find which launcher does each thing best, bring the
others up to it, and move anything now duplicated into the SDK.
