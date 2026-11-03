# Research: @GAME@

What the community already solved, found before writing any fix
(rea-launcher-sdk `docs/STANDARDS.md`, "Research before building"), and
what the game's own files showed (REA, read in place). Keep observations,
inferences and decisions apart.

## Where we looked

- [ ] PCGamingWiki: the game's page (fixes, widescreen, controller, known issues, Linux)
- [ ] Widescreen Fixes Pack / ThirteenAG, Silent's projects
- [ ] GitHub: "@GAME@" + fix / patch / widescreen / tweaks
- [ ] ModDB, Nexus Mods
- [ ] Steam and GOG forums, fan and speedrunning communities
- [ ] ProtonDB, WineHQ AppDB

## Community projects

| Project | What it fixes | Licence | Maintained | Game version it needs | Decision |
|---|---|---|---|---|---|
| (name, link) | | | yes / no (last release) | | bundle / configure / download on request / not used: why |

Decisions: **Bundle**, **Configure**, **Download on request**, **Not
used** (why), or **Our own** (draft C1): a fix nobody had, which gets a
draft in `docs/CONTRIBUTIONS.md` with that id, or `no draft: <reason>`
(`sdk/scripts/launcher-check`, rule `own-fix-draft`).

## Owner decisions

Licence-less assets, official art, DRM, anything public: the owner's call,
dated, with the reason. A build with its DRM or disc check removed
(pre-activated, no-CD, cracked) is supported only by such a decision; the
launcher never applies, ships or patches DRM files, and the README says so
(`sdk/docs/STANDARDS.md`, "DRM exceptions"). Record it here and in
`AGENTS.md`, then add the `drm-exception` line to `.rea-check-waivers`.

| Date | Decision | Why |
|---|---|---|
| | | |

## Editions

| Build | Source | @GAME_EXE@ SHA-256 | Size | Notes |
|---|---|---|---|---|
| | retail / GOG / Steam / patched | | | |

## The game's files (static analysis)

- **Renderer:** (D3D7/8/9, OpenGL, DirectDraw)
- **Input:** (DirectInput, XInput)
- **Audio:** (DirectSound, DirectSound3D/EAX, CD audio)
- **DRM, disc checks:**
- **Files and registry keys it writes:**
- **Intro videos and codecs:**

## Old-game trouble spots

| Spot | Applies? | Finding | Fixed by |
|---|---|---|---|
| Timers and frame pacing on fast CPUs | | | |
| Multi-core affinity | | | |
| DirectSound3D / EAX audio | | | |
| CD audio | | | |
| Intro-video codecs (Bink, Indeo, WMV) | | | |
| Resolution and aspect tables | | | |
| Focus loss | | | |

## The problems the launcher will solve

One line each, agreed with the owner before building:

1.
