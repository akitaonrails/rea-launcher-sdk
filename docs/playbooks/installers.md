# Playbook: a player points the launcher at an installer

Some players have the game's installer, not an installed game: GOG's
offline installer, or a repack. The launcher unpacks it once into a folder
of its own and plays from there, so the player never runs a setup program.
The code is `rea_launcher::installer` (identify, plan, finish),
`rea_launcher_win32::innoextract` (the process and the whole flow) and
`rea_launcher_ui::unpack` (the progress window). CMR2005 did this first
for GOG's installer.

## Never run an installer

Installers open dialogs nobody answers, some hang under Wine (GOG's Colin
McRae Rally 2005), and they run code nobody checked. The launcher reads
Inno Setup installers with the bundled **innoextract 1.9** instead
(`components/innoextract.exe`, the official Windows build, zlib licence).
It is the same on Windows and Linux: the AppImage runs the Windows launcher
in its Wine, and innoextract.exe with it. The installer files are only
read; innoextract writes into a work folder beside the launcher's copy.

Out of scope: InstallShield discs and anything that needs a chain of patch
installers on top (DiRT 2007), and games with no installer at all (CMR1).
Those players install the game the usual way.

## 1. Research and decide

- Record each installer in `docs/RESEARCH.md`: where it comes from, its
  setup program's SHA-256, each data part's name and size, the Inno Setup
  version (`innoextract --info`), and what it installs.
- **Repacks are third-party builds.** Supporting one (CMR3's and CMR4's
  Magipack repacks) is the owner's decision, recorded with its date in
  `RESEARCH.md` ("Owner decisions") like any edition. A repack that
  removes DRM falls under STANDARDS "DRM exceptions": ask first.
- **Identify by SHA-256 and part sizes.** The setup program is hashed; the
  data parts (gigabytes) are checked by exact size only, since innoextract
  checks every file's checksum while it unpacks (`--test`).

## 2. Verify the unpacked game against an installed copy

Before adding an installer to the table, unpack it with the Linux
innoextract (in the `gaming` box) into `.work/` and compare it with an
installed copy, byte for byte:

```sh
innoextract --extract --test --exclude-temp --collisions=rename \
  --include app --output-dir .work/inst/<id> "<installer folder>/<setup>.exe"
diff -r .work/inst/<id>/app "<installed game>"     # or: rsync -rcn --delete
```

Read every difference:

- **Collisions.** Two components that install the same file come out as
  `X` and `X#<component>` (`--collisions=rename`). CMR4's repack has an
  optional livery (`base\p206`, off by default) that writes the same
  `p206.big` files as the default component `base`: the default copy is
  `p206.big#base`. List the components whose copy wins in
  `InstallerBuild::prefer` (`&["base"]`); after the unpack each `X#base`
  replaces `X` and any other leftover beside an `X` is removed (both are
  logged). Never use `--collisions=rename-all`: it makes names with
  backslashes (`p206.big#base\p206`), which break on Windows.
- **Files the repack added** that the launcher bundles itself (CMR3's
  repack: `dinput8.dll`, `SilentPatchCMR3.asi`; CMR4's: `D3D9.dll`,
  `dgVoodoo.conf`, `dgVoodooCpl.exe`). The launcher would keep them as the
  player's own files and the repack's older copy would win, so list them
  in `InstallerBuild::drop`; they are removed before the game check.
- Anything else that differs: find out why before supporting the
  installer.

With `prefer` and `drop` set, the unpacked folder must equal the installed
copy. Commit nothing from it, and never write into the installer's folder.

## 3. Adopt it in the launcher

```rust
use rea_launcher::identify::Known;
use rea_launcher::installer::{self, InstallerBuild};

pub static INSTALLERS: &[Known<InstallerBuild>] = &[Known::new(
    "<setup sha256>",
    InstallerBuild {
        name: "the Magipack repack",           // the player reads this
        setup: "Colin McRae Rally 04_Repack_Setup.exe",
        parts: &[("Colin McRae Rally 04_Repack_Setup-1.bin", 770_000_000) /* … */],
        unpacked: 2_000_000_000,                // bytes, for progress and space
        root: "app",
        prefer: &["base"],
        drop: &["D3D9.dll", "dgVoodoo.conf", "dgVoodooCpl.exe"],
    },
)];
```

1. Look for an installed game first. When there is none, call
   `installer::detect(dir, INSTALLERS, UNKNOWN_EDITION_ISSUE)`: `Ok(None)`
   is the usual "no game here" message; `Err` is shown as any play-time
   error (an unknown installer is left alone and reported with its
   SHA-256; a missing or short part is named).
2. Choose the destination: a folder of the launcher's own beside the
   runtime copy (CMR2005: `<data>\unpacked`), never inside the game
   folder or the installer's folder.
3. Run it, with the progress window when one can open:

   ```rust
   let check = |dir: &Path| edition::find(dir);    // the launcher's own game check
   let mut log = |line: &str| out.log.line(line);
   let exe = components_dir.join(innoextract::EXE);
   let result = innoextract::unpack(&exe, &found, &dest, check, &mut log, |job| {
       let state = unpack::Unpacking::new(GAME_TITLE, found.build.name, found.build.unpacked);
       unpack::show(&PALETTE, &METRICS, unpack::Window { title: WINDOW_TITLE, fit, hero_jpeg: HERO_JPEG, photo_width: PHOTO_W },
                    state, job, || xinput::buttons())
   })?;
   ```

   `Unpacked::Ready(done)`: the game is in `dest`; `Unpacked::Cancelled`:
   the player stopped it, exit quietly. With `--no-window`, pass a `watch`
   that returns `Err(..)`: the unpack is waited for without a window.
4. **Point the game setting at the unpacked folder.** Play from it, save
   it as `game` in the launcher's settings file (the one the settings
   window writes), and tell the player in `launcher.log` and the window:
   "The game was unpacked into <dir>. The game folder setting now points
   there; the installer files are no longer needed." When the folder came
   from the command line (`--game`), don't save; only say where the game
   is. A player who points at the installer folder again still gets the
   unpacked copy reused (`installer::begin` returns `Begin::Done` when the
   folder's `.rea-unpacked` marker names this installer's SHA-256).
5. Everything after that (runtime copy, fixes, saves) runs on the unpacked
   folder as on an installed game.

The window says what happens and from which installer, that it happens
once, that the installer is not run and its files are only read, and how
much is left. "Stop unpacking" (also Escape, the pad's B, closing the
window) stops innoextract and removes the work folder.

## Safety rules the code keeps

- Only a folder with the `.rea-unpacked` marker is ever replaced or
  removed; a destination (or a work folder) the player made is refused with
  a message. A new installer build replaces the launcher's own folder.
- The launcher's game check runs on the unpacked files before they take the
  destination's place; a refused game is removed with the work folder.
- **Disk space:** `installer::check_space` asks for the game's size plus a
  tenth before innoextract starts, and says how much is needed and free.
- Cancel and failure leave no partial game behind.

## Packaging

In `packaging/third-party.txt`, the two pin lines that
`sdk/packaging/innoextract.sh`'s `innoextract_pins` prints; in
`package.sh`:

```sh
. sdk/packaging/pins.sh
. sdk/packaging/innoextract.sh
PIN_FILE=packaging/third-party.txt
innoextract_bundle "$pkg" "$work"   # components/innoextract.exe + licenses/innoextract/*.txt
```

Allow both paths in `packaging/package.allow`, pass `--payload components
--payload licenses` to `appimage-wine.sh`, and list innoextract 1.9 in
`THIRD_PARTY.md` (zlib; built with Boost, liblzma and libstdc++, whose
texts ship in `licenses/innoextract/`). `launcher-check` rule
`innoextract` checks the pins and the credit.

## Tests

- Host: an `INSTALLERS` table test, and the launcher's wiring (installer
  folder → `detect` → unpack → the game check) with a fake installer as in
  `rea_launcher::installer`'s tests (sparse parts, a fake `run`).
- Real installers stay outside the repository: an env-gated test may unpack
  the owner's copy into `.work/` (never the installer's folder) and compare
  with an installed copy.
- `RUNTIME_NOTES.md`: the first-start unpack checked on Windows and in the
  AppImage, with the time it took.
