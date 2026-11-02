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
