<!--
  For agents: follows sdk/docs/playbooks/readme.md ("linux/README.md").
  Fill the TODOs, keep the headings, then run this file through the
  `humanizer` skill (wording only) and delete this comment.
-->

# @GAME@ on Linux

The launcher is a Windows program (the same one as on Windows); on Linux
it runs through Wine or Proton. Three ways to play:

## The AppImage

```sh
chmod +x @LAUNCHER_NAME@-*-x86_64.AppImage
./@LAUNCHER_NAME@-*-x86_64.AppImage
```

It brings its own Wine, so nothing else needs to be installed. The
settings window opens: press **Browse…** and pick your game folder (your
Linux files are on drive `Z:`), choose a look and press **Play**. When
[gamescope](https://github.com/ValveSoftware/gamescope) is installed the
game runs in it; otherwise in a plain window. Its Wine prefix is
`~/.local/share/@ID@-launcher/prefix`.

## Steam (Proton)

1. Unzip the release, for example into `~/Games/@ID@-launcher`.
2. In Steam: **Games → Add a Non-Steam Game to My Library…** and pick
   `@LAUNCHER_EXE@`.
3. **Properties → Compatibility**: force a Proton version.
4. Press **Play**, pick your game folder (under drive `Z:`), choose a look
   and press **Play**.

### Steam Deck

Follow the steps above in Desktop Mode, then play from Game Mode. The
window recommends the **Steam Deck** look and works with the controls.

<!-- TODO: say whether Game Mode has been verified. -->

## Your own Wine

`linux/@ID@-play` runs the launcher and the game in a private Wine prefix,
inside gamescope when it is installed. It needs `rea-play.sh` beside it
(the zip and the AppImage ship it) or in `../sdk/` (a repository checkout).

```sh
linux/@ID@-play             # the settings window, then the game
linux/@ID@-play --play      # the game with the saved settings, no window
linux/@ID@-play stop        # end the game
linux/@ID@-play status      # running or idle
```

Launcher options after `--play` apply to that run, for example
`linux/@ID@-play --play --look original`. When Wine and gamescope live in
a [distrobox](https://github.com/containers/distrobox), run it through the
container:

```sh
@PREFIX@_RUN="distrobox enter gaming --" linux/@ID@-play
```

## Environment

| Variable | Meaning |
|---|---|
| `@PREFIX@_PREFIX` | the Wine prefix (default `~/.local/share/@ID@-launcher/prefix`) |
| `@PREFIX@_LAUNCHER` | `@LAUNCHER_EXE@` (default: beside the script, or one folder up) |
| `@PREFIX@_RUN` | a wrapper for wine and gamescope, e.g. `distrobox enter gaming --` |
| `@PREFIX@_GPU` | GPU environment instead of the detected one (empty: none) |

## Where things are

| What | Where |
|---|---|
| Settings (`launcher.cfg`) and log (`launcher.log`) | in the prefix, `drive_c/users/<you>/AppData/Local/@LAUNCHER_NAME@/` |
| The launcher's copy of the game | `…/AppData/Local/@LAUNCHER_NAME@/runtime` |
| Your saves | TODO |
| `@ID@-play`'s logs and lock | `~/.config/@ID@-launcher` |

## Troubleshooting

<!-- TODO: Linux-only problems found in testing, as bold symptom + fix. -->

- **Anything else:** run `@LAUNCHER_EXE@ --check` through the AppImage or
  `@ID@-play`, and look at `launcher.log` (see the table above).
