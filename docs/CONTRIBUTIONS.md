# Contributing findings back to the community

Drafts of what the REA launchers found that the wider community could use:
PCGamingWiki (PCGW) first, then upstream projects, ProtonDB and the WineHQ
AppDB.

**Rule: the owner reviews and posts. Agents only draft.** Nothing in this
file has been submitted anywhere. A draft is posted only after the owner
approves it by number ("post #C3"), following
`docs/playbooks/contribute-back.md`; then record the link and date in its
status. PCGW wikitext is pasted by hand (the site is behind a Cloudflare
challenge), and ProtonDB/AppDB are web forms. **WineHQ AppDB accepts
plain-Wine results only**, not runs with DXVK, dgVoodoo or Xidi, so the
wrapper-based Wine findings go to PCGW Linux notes instead. Before posting,
the owner:

1. checks the live page again (the survey below was read through Wayback
   snapshots from Oct 2025 to Oct 2026, because PCGW and AppDB block
   automated clients);
2. fills in `{{Refcheck|user=…}}` with their PCGW user name and the date;
3. checks each hex edit on a copy of the exact executable named in the
   draft (same SHA-256), never on another build;
4. never attaches game files, patched executables or saves.

Only findings verified in the real game (per each repo's
`docs/RUNTIME_NOTES.md`) are marked **ready**. Findings that rest on code,
static analysis or headless runs only are listed under "After
verification", with what is still missing.

Prepared 2026-11-02 from: `rea-screamer` (`crates/screamer-launcher/src/fixes.rs`,
`docs/04-…`, `docs/RUNTIME_NOTES.md`), `rea-colin-mcrae-3`, `rea-colin-mcrae-4`
(`src/patch.rs`, `close-hook/`, `docs/RESEARCH.md`), `rea-sega-rally-revo`
(`crates/srr-launcher/src/patches.rs`, `docs/RESEARCH.md`), and `rea-outrun-2006`.

## Offsets: file offset, not VA

PCGW hex fixes give the **file offset** you type into a hex editor's
"Go to" (HxD: Ctrl+G), shown as `0028CD30` with the bytes in `<code>`.
The launchers store patch sites in different forms:

- **Screamer trilogy:** `fixes.rs` uses **LE virtual addresses**, not file
  offsets. These are Watcom/DOS4GW LE executables, so VA → file offset
  depends on the object and page tables. Every file offset in this
  document was computed from the LE page map (a read-only script
  replicating `le.rs::va_to_file_offset`). The expected original bytes
  were then read back from the real GOG/ISO executables and all matched.
- **CMR04:** the patch is a 6-byte signature search. The file offset below
  was located in the launcher's runtime copy of `cmr4.exe` (PE, `.text`,
  file offset = VA − 0x400000).
- **Sega Rally Revo:** `patches.rs` already works in file offsets for
  `.rdata` (the aspect tables are found by pattern). The offsets were
  re-read from the runtime copy of `SEGA Rally_SSE1.exe`.

## Summary

| # | Finding | Ours or community | Target | Status |
|---|---|---|---|---|
| S1 | Screamer (1995): Hor+ 16:9 widescreen hex edit (camera matrix X-row × 0.75), `STARTH.EXE`/`STARTL.EXE` (GOG) | ours | PCGW *Screamer*, Video → Widescreen | **ready**, once the owner confirms the 1080p gameplay recording (Enhanced preset) shows Hor+ and not a stretch |
| S2 | Screamer 2 / Screamer Rally: the same Hor+ edit (6 executables) | ours | PCGW *Screamer 2*, *Screamer Rally* | needs verification (no in-race visual check on record) |
| S3 | Screamer (1995) runs a fixed 50 Hz game loop (2 ticks of a 100 Hz timer); physics comes from a 50-entry table, so unlocking breaks it | ours | PCGW *Screamer*, the 60/120 FPS rows (the wiki says 60 FPS true, "70 FPS cap") | **ready** as a correction note; owner to confirm in-game with an FPS counter |
| S4 | Screamer 2/Rally Glide mode in DOSBox Staging exits with `MAXCACHE` when `voodoo_memsize` is above 4 | ours | PCGW *Screamer 2*, *Screamer Rally*, Issues fixed | **ready** (exit reproduced, fix is a config value) |
| S5 | Screamer 2 (GOG): the bundled `glide2x.ovl` is a 31 KB Windows/nGlide stub; Glide mode in plain DOSBox needs Screamer Rally's 348 KB DOS `GLIDE2X.OVL` | ours | PCGW *Screamer 2*, API/Glide note | **ready** (3dfx mode entered with the genuine OVL); an in-race session is advisable |
| S6 | Screamer 2/Rally: `grTexDownloadMipMap: mipmap cannot span 2 Mbyte boundary` exit (F7 hex fix) | ours | PCGW Issues fixed | needs verification (the bug is reproduced; the fixed exe was not shown surviving the same track-preview sequence) |
| S7 | Remove the 10% joystick deadzone (E5) | ours | PCGW Input | needs verification (the launcher's own pad injection bypasses this code, so E5 was never tested with DOSBox's gameport joystick) |
| S8 | Extended draw distance (E6) | ours | PCGW / mod note | needs verification (headless race segment only, no visual check) |
| S9 | SVGA/VESA/SETUP fixes F2, F3, F5, F6 (DPMI probe, WinGranularity, VBE mode 0x120, SETUP divide-by-zero at high cycles) | ours | PCGW Issues fixed (F3 may explain the known SVGA track-5 crash) | needs verification (no before/after run recorded; the launcher never runs SETUP) |
| C1 | CMR3: a new profile has no controller selected and the default bindings put steering on the right stick | ours | PCGW *Colin McRae Rally 3*, Input | **ready**; adds to an existing note |
| C2 | CMR3 under Wine/Proton: WineD3D gives about 10 FPS at 4K (use DXVK); exclusive fullscreen picks the wrong (portrait) monitor and rewrites WIDTH/HEIGHT/ADAPTER (use SilentPatch borderless or gamescope) | ours | PCGW (no Linux notes yet); AppDB 7563 only with a plain-Wine run | **ready** for PCGW; AppDB needs a plain-Wine (no DXVK) run |
| C3 | CMR04: AuToMaNiAk005's 16:9 fix (Low HUD) is meant for a **4:3 render stretched to 16:9**; at a native 16:9 render the picture is squeezed 25% | ours (insight); the bytes are the existing community fix | PCGW *Colin McRae Rally 04*, Widescreen | **ready** as an `{{ii}}` note, unless the fix's own readme already says to use a 4:3 mode (owner to check) |
| C4 | CMR04 ignores Alt+F4 and window-close requests (WndProc returns 0 for `WM_CLOSE`, main loop ignores `WM_QUIT`) | ours | PCGW Issues unresolved | **ready for Linux/Wine**; needs a Windows check before claiming it there |
| C5 | CMR04: analogue pedals on the triggers with Xidi (one combined pedal axis) | ours (mapping) | PCGW Input fixbox | **ready** (stages driven, 8BitDo Ultimate 2) |
| C6 | CMR04 under Wine: needs `AUDIO_PATH`/`CD_PATH`/`INSTALL_PATH`/`VIDEO_PATH` registry values in a fresh prefix; dgVoodoo2 2.80 + DXVK works where 2.87.6 crashed; a portrait primary monitor needs dgVoodoo `FullScreenOutput` | ours | PCGW Linux note (AppDB not eligible: wrappers) | **ready** |
| R1 | Sega Rally Revo: stuken's SegaRallyRevoStutterFix fixes the 16/17 ms jitter and the Championship stage-2 stutter (the PCGW FPS fixbox names no tool) | community; we verified it | PCGW *Sega Rally Revo*, Frame rate | **ready** (the patched hash matches the reference; races driven) |
| R2 | Sega Rally Revo: ultra-wide aspect-table offsets for the clean `SEGA Rally_SSE1.exe` v4.0.6.0 | community technique; our offsets for this build | PCGW Ultra-widescreen | needs verification (no 21:9 screen tested) |
| R3 | Sega Rally Revo under Wine: d3dx9/directplay, the saved-game layout, the Wine virtual desktop on multi-monitor/high-refresh hosts, slow logo videos, DXVK on hybrid GPUs | ours | PCGW Linux note; AppDB 7058 (retail rated Garbage) only after a plain-Wine run | **ready** for PCGW |
| O1 | OutRun2006Tweaks under Wine/Proton: `InputBackend = 0` (Windows.Gaming.Input) loses the d-pad in menus; `InputBackend = 3` (XInput) works | ours | PCGW *OutRun 2006*, Linux note; upstream issue to emoose/OutRun2006Tweaks | **ready** (owner play-test) |
| X1 | `windows-sys` raw-dylib `kernel32` imports dropped when linking with GNU ld 2.47 (mingw-w64): the exe crashes at the first call | ours | upstream (binutils or windows-rs), optional | ready to file after a minimal reproducer is made |
| — | Screamer "60/120 FPS via smoothed_dt clamp + SOS timer" | — | — | **do not publish**: the code ships E1 as a no-op because every such unlock broke physics (see S3). The 120/240 Hz timer idea in `docs/04-…` was never tested |
| — | CMR3 `CMR3` profile format (CRC-32/BZIP2 at 0x42A24, controller block offsets) | ours | — | not wiki material; keep in the repo |

Already documented, so nothing to propose: SilentPatch for CMR3 (widescreen,
FOV, borderless, high-FPS issue); the CMR04 registry widescreen and
AuToMaNiAk005 fix; Widescreen Fixer for Sega Rally Revo ultra-wide;
OutRun2006Tweaks for everything video-related on OutRun, plus the PCGW DXVK
`d3d9=n,b` Linux fixbox.

---

## Ready drafts: PCGamingWiki

House style follows existing PCGW fixboxes (RalliSport Challenge hex fix,
CMR3 SilentPatch fixbox): `{{Fixbox|description=…|ref=…|fix=…}}`, numbered
steps, `{{file|…}}`, `{{folder|{{p|game}}}}`, offsets as plain hex without
`0x`, bytes in `<code>`. The repository link is the source; no game files.

### S1 — Screamer (1995): Hor+ widescreen

Page: <https://www.pcgamingwiki.com/wiki/Screamer>. Change the Video
template rows and add a `===Widescreen resolution===` section.

```wikitext
|widescreen resolution      = hackable
|widescreen resolution notes= 3D view only; menus and HUD stay 4:3 and are stretched. See [[#Widescreen resolution|Widescreen resolution]].
```

```wikitext
===[[Glossary:Widescreen resolution|Widescreen resolution]]===
{{Fixbox|description=Hor+ 16:9 hex edit|ref=<ref>{{Refcheck|user=OWNER|date=2026-11-XX|comment=GOG release, DOSBox Staging 0.82.2 with aspect = stretch.}}</ref><ref>{{Refurl|url=https://github.com/akitaonrails/rea-screamer-trilogy/blob/master/crates/screamer-launcher/src/fixes.rs|title=rea-screamer-trilogy – fixes.rs (E2)|date=2026-11-02}}</ref>|fix=
{{ii}} Narrows the camera's horizontal axis to 3/4, so a 16:9 display shows the original 4:3 view in the centre plus more to the sides (horizontal FOV about 90° → 106°). Menus and HUD are 2D and get stretched.
{{ii}} Only for the executables below (GOG release); check the SHA-256 first. Back up the file.
# Open the executable with a hex editor such as [https://mh-nexus.de/en/downloads.php?product=HxD HxD].
# Press {{key|Ctrl|G}}, go to the first offset and replace the 31 bytes:
#* Original: <code>0F BF C9 0F BF F6 0F BF DB 0F BF FF 0F BF ED 0F BF C0 D1 F9 D1 FE D1 FB D1 FF D1 FD D1 F8 52</code>
#* New: <code>8B 06 C1 F8 02 29 06 8B 46 0C C1 F8 02 29 46 0C 8B 46 18 C1 F8 02 29 46 18 2B C0 89 46 24 C3</code>
# Go to the second offset and replace the 5 bytes <code>2B C0 89 46 24</code> with <code>E8 5B 0A 00 00</code>.
#* {{file|STARTH.EXE}} (SVGA, 1,034,923 bytes, SHA-256 <code>1246afc95463f96598d5fee764fc99c847b6d3e0bf8ca2268bd362c664bb28a3</code>): offsets '''095F22''' and '''0954C2'''.
#* {{file|STARTL.EXE}} (VGA, 1,034,919 bytes, SHA-256 <code>ad20fa3919ecf86da5931e2e0e495d4043f6be8c44e57299114528d87ee27ec7</code>): offsets '''095C72''' and '''095212'''.
# Make DOSBox stretch the picture to 16:9: in DOSBox Staging set <code>aspect = stretch</code> in <code>[render]</code> and a 16:9 <code>windowresolution</code>/fullscreen.
}}
```

Notes for the owner:
- The first edit writes a small routine into an unreferenced duplicate
  camera function (`SetCameraMatrix_Unused`). The second turns
  `sub eax,eax; mov [esi+24h],eax` into a `call` to it. The routine
  does `x -= x>>2` on matrix entries `[esi+0]`, `[esi+0Ch]`, `[esi+18h]`
  and then repeats the two displaced instructions. The `call` rel32 is
  relative and the patch range has no LE fixup records, so no relocation
  edit is needed (the launcher checks this before patching).
- Only DOSBox Staging 0.82.2 with `aspect = stretch` was used. The GOG
  bundle's DOSBox (0.74 family) presumably needs `aspect=false`, a 16:9
  `fullresolution` and `output=opengl`, but that is **untested**: say so or
  leave it out.
- Status: I found no explicit in-race check of E2 in `RUNTIME_NOTES.md`.
  The evidence is the README's 1080p gameplay video recorded with the
  Enhanced preset (widescreen defaults to on). The owner should confirm the
  video shows Hor+ (cars not stretched) before posting.

### S3 — Screamer (1995): the frame-rate rows

The wiki currently says `60 fps = true`, `120 fps = false`, "70 FPS cap".
Static analysis says the race loop waits for 2 ticks of the HMI SOS timer
at 100 Hz (`STARTH.EXE` file offset 02A446: `83 FA 02 72 F5`, `cmp edx,2; jb`),
which is a 50 FPS ceiling. Physics then indexes a 50-entry table by
`int(1/dt)`, with `dt` clamped to ≥ 0.02 s. The repo's runtime tests showed
that lowering either limit breaks the game: the game runs 1.4–2× too fast,
or the track crashes on load.

**Before posting, verify with an FPS counter in a race** (DOSBox Staging
shows FPS in the title bar), because this contradicts the wiki's 60/70 FPS
claim. If confirmed:

```wikitext
|60 fps                     = false
|60 fps notes               = Races run at a fixed 50 FPS (two ticks of a 100 Hz timer); physics uses a 50-entry per-frame-rate table, so the cap cannot be raised without breaking the game.<ref>{{Refcheck|user=OWNER|date=2026-11-XX|comment=Measured in DOSBox Staging; analysis in https://github.com/akitaonrails/rea-screamer-trilogy/blob/master/docs/04-screamer-engine-and-patches.md}}</ref>
|120 fps                    = false
|120 fps notes              =
```

Screamer 2 is already "50 FPS cap" on PCGW, which agrees with our analysis.
Screamer Rally says "60 FPS cap". In our analysis Rally's spin-wait is
already 1 tick, while the `dt ≥ 0.02 s` physics floor remains. What that
means for its real frame rate is **not established**, so do not edit the
Rally row.

### S4 — Screamer 2 / Screamer Rally: `MAXCACHE` exit in Glide mode

Pages: *Screamer 2*, *Screamer Rally*. Add under Issues fixed:

```wikitext
===Game exits with "MAXCACHE" in 3dfx mode===
{{ii}} With emulated Voodoo memory above 4 MB, the 3dfx executable ({{file|S2_3DFX.EXE}} / {{file|STRT3FX.EXE}}) uploads more than 1,024 textures before its texture cache wraps, prints <code>MAXCACHE</code> and quits.
{{Fixbox|description=Emulate a 4 MB Voodoo 1|ref=<ref>{{Refcheck|user=OWNER|date=2026-11-XX|comment=DOSBox Staging 0.82.2; voodoo_memsize = 12 reproduces the exit, 4 avoids it. Analysis: https://github.com/akitaonrails/rea-screamer-trilogy/blob/master/docs/RUNTIME_NOTES.md}}</ref>|fix=
# In the DOSBox Staging configuration, set <code>voodoo_memsize = 4</code> in the <code>[voodoo]</code> section (2 MB frame buffer + 2 MB texture memory, as on a real Voodoo 1).
}}
```

Note: this applies to DOSBox builds with built-in Voodoo emulation (Staging
0.81+). It does not apply to the GOG/Steam bundles, which use nGlide.

### S5 — Screamer 2 (GOG): Glide in plain DOSBox

Page: *Screamer 2*, Other information → API, as a note on the Glide row:

```wikitext
{{ii}} The GOG/Steam {{file|glide2x.ovl}} (31,425 bytes) is a stub for the bundled nGlide setup and does not work with DOSBox's own Voodoo emulation. Copying the 348,791-byte DOS {{file|GLIDE2X.OVL}} from a [[Screamer Rally]] install next to {{file|S2_3DFX.EXE}} lets the 3dfx renderer run in DOSBox Staging.<ref>{{Refcheck|user=OWNER|date=2026-11-XX}}</ref>
```

Hashes measured on this machine (the repo disagrees with itself; see
"Repo inconsistencies"): stub `fe6f33d3add608eee61f956b82eaa9c9027ee79441dde29b3fbc687f0605f37b`;
Screamer Rally ISO `GLIDE2X.OVL` `6981a7bd77445be7b360ca367af1e527927db396546db15b69803b16a67676a9`.
Evidence: the headless run entered "3dfx Voodoo 640x480 16-bit" with
`S2_3DFX.EXE` (`RUNTIME_NOTES.md` §5). An in-race session before posting is
advisable. Whether to suggest copying a file between two games is the
owner's call: both are owned copies, and nothing is downloaded.

### C1 — Colin McRae Rally 3: controller on a new profile

Page: <https://www.pcgamingwiki.com/wiki/Colin_McRae_Rally_3>. The
`full controller notes` already say the keyboard is needed "to reconfigure
the controller". This draft makes the cause concrete. Add an Issues fixed
section:

```wikitext
===Controller does nothing in a new profile===
{{ii}} A newly created profile has no controller selected, and the game's default joystick bindings put steering on the right stick.<ref>{{Refcheck|user=OWNER|date=2026-11-XX|comment=Patch 1.1, DRM-free exe, SilentPatch 2.1, Xidi 4.3.1.}}</ref>
{{Fixbox|description=Select and rebind the controller|fix=
# With the keyboard, go to ''Options → Controls'' and choose the controller as Player 1's device.
# Rebind steering to the left stick and the pedals to the triggers (with [https://github.com/samuelgr/Xidi Xidi] both triggers get separate axes, so left-foot braking works).
}}
```

Optional technical detail, for the repo link only: the profile `CMR3`
stores the device index at 0x1DB98 and vibration at 0x1DB94, with a
CRC-32/BZIP2 at 0x42A24. Steering lives in words 9–10 and pedals in words
23–24 of the joystick block; the axis codes change with the pad layout
(12/13 without a hat, 14/15 with one).

### C2 — Colin McRae Rally 3: Linux / Wine / Proton

PCGW has no Linux notes for CMR3. Add to Issues fixed (or Other
information, following the OutRun page's "Linux/SteamOS" pattern):

```wikitext
===Low frame rate and wrong monitor under Wine/Proton===
{{Fixbox|description=Use DXVK and borderless mode|ref=<ref>{{Refcheck|user=OWNER|date=2026-11-XX|comment=Wine 11.20/11.21, RTX 5090, multi-monitor Hyprland (XWayland). Details: https://github.com/akitaonrails/rea-colin-mcrae-3/blob/master/docs/RUNTIME_NOTES.md}}</ref>|fix=
{{ii}} With Wine's built-in Direct3D (OpenGL) the game runs at about 10 FPS at 4K; through [https://github.com/doitsujin/dxvk DXVK] it runs at full speed.
{{ii}} In exclusive fullscreen Wine can choose another monitor (e.g. a portrait one) and crop the picture; the game also writes that monitor's WIDTH/HEIGHT/ADAPTER back to its settings.
# Put DXVK's 32-bit {{file|d3d9.dll}} next to {{file|Rally_3PC.exe}} and use the DLL override <code>d3d9=n,b</code> (keep <code>dsound=n,b</code> for [[#SilentPatch|SilentPatch]]).
# Use SilentPatch's borderless display mode, or run the game in fullscreen inside gamescope.
}}
```

ProtonDB: not applicable (not on Steam). AppDB 7563 (currently Silver,
Wine 4.15) accepts plain-Wine results only. The 10 FPS WineD3D observation
*is* a plain-Wine result, but the playable configuration used DXVK, Xidi
and SilentPatch. File an AppDB report only after a plain-Wine run, and
then only the plain-Wine facts. Below is the text as a PCGW/forum note,
not an AppDB report:

> Wine 11.20 (WoW64), DRM-free 1.1 exe + SilentPatch 2.1. Gold with DXVK:
> installs and runs at 4K at full frame rate. Without DXVK, about 10 FPS at
> 4K. Exclusive fullscreen picks the wrong monitor on multi-monitor setups:
> use SilentPatch borderless or gamescope. A fault in Wine's quartz video
> playback can bring up a crash dialog (`winetricks nocrashdialog` hides it;
> the game continues). A pad works through Xidi (DirectInput wrapper).

(The rating is the owner's choice. A rare in-race page fault inside DXVK's
d3d9 is still unexplained, so Gold rather than Platinum.)

### C3 — Colin McRae Rally 04: use a 4:3 mode with the 16:9 fix

Page: <https://www.pcgamingwiki.com/wiki/Colin_McRae_Rally_04>, the "Use
widescreen fix" fixbox (AuToMaNiAk005's fix with dgVoodoo 2). Its "16:9
Low" option changes exactly one float in `cmr4.exe`, the camera's
horizontal factor 1.0 → 0.75:

- file offset **077829**: `80` → `40` (in `C7 44 24 04 00 00 80 3F E8`, i.e.
  `mov dword [esp+4], 1.0f` → `0.75f`; function at VA 0x477820);
- measured on `cmr4.exe` 3,756,032 bytes, FileVersion 1.0.0.0, SHA-256
  before the edit `7dca9f4cab16bc1a98df8871f3d20aec4cd931a818ea0899db86c83c1f712b41`
  (derived by reverting that one byte in the launcher's runtime copy; the
  launcher changes no other byte. PCGW says the fix "requires 1.01", yet the
  version resource reads 1.0.0.0. The owner should check which patch level
  this copy is).

Suggested `{{ii}}` line to add inside that fixbox (do not post a separate
hex edit, since the bytes are the community fix):

```wikitext
{{ii}} The 16:9 setting assumes a 4:3 picture stretched to 16:9. Set a 4:3 resolution in the game (e.g. 1440x1080 for a 1920x1080 screen, 2880x2160 for 3840x2160) and set dgVoodoo's scaling mode to ''Stretched''; at a native 16:9 resolution cars and scenery come out about 25% too narrow. The HUD remains drawn on a 640x480 grid and looks slightly wide.<ref>{{Refcheck|user=OWNER|date=2026-11-02|comment=Measured on a car's rear: width/height 0.88 at native 16:9, 1.21 with a 4:3 render stretched. https://github.com/akitaonrails/rea-colin-mcrae-4/blob/master/docs/RESEARCH.md}}</ref>
```

Verified in game (RUNTIME_NOTES 2026-11-02, user-confirmed). Before
posting, check whether the fix's own readme already says to use a 4:3 mode.
If it does, this is "already documented" and the line is unnecessary.

### C4 — Colin McRae Rally 04: Alt+F4 does nothing

```wikitext
===Game cannot be closed with Alt+F4 or the window's close button===
{{ii}} The game's window procedure ignores close requests and its main loop ignores <code>WM_QUIT</code>, so Alt+F4, the close button (and a window manager's close action under Linux) do nothing. Quit from the main menu, or end {{file|cmr4.exe}} with Task Manager.<ref>{{Refcheck|user=OWNER|date=2026-11-XX|comment=Observed under Wine; analysis and a close hook: https://github.com/akitaonrails/rea-colin-mcrae-4/tree/master/close-hook}}</ref>
```

Status: observed under Wine (Super+W did nothing before the hook). The
cause is in the game's own WndProc (0x501EF0 returns 0 for `WM_CLOSE`) and
main loop (0x503000), so it should apply on Windows too, **but that is
untested**. Check Alt+F4 on Windows before posting it as a general issue;
otherwise post it under the Linux note only. Also note: in the hook's
testing Alt+F4 itself was not separately exercised (SC_CLOSE was).

### C5 — Colin McRae Rally 04: analogue pedals on the triggers (Xidi)

PCGW's Input rows point at the "force feedback restore module" for XInput.
Here is an alternative verified fixbox for analogue triggers:

```wikitext
{{Fixbox|description=Use Xidi for analogue trigger pedals|ref=<ref>{{Refcheck|user=OWNER|date=2026-11-02|comment=Xidi 4.3.1, 8BitDo Ultimate 2; full stages driven.}}</ref>|fix=
{{ii}} The game reads one DirectInput axis for both pedals. This mapping puts RT/LT on that axis and keeps the stick and buttons in the standard layout.
# Download [https://github.com/samuelgr/Xidi/releases Xidi] and copy its 32-bit {{file|dinput8.dll}} next to {{file|cmr4.exe}}.
# Create {{file|Xidi.ini}} next to it with:
<pre>
[Mapper]
Type = CMR04Gamepad

[CustomMapper:CMR04Gamepad]
Template = StandardGamepad
StickLeftX = Axis(X)
StickLeftY = Null
TriggerRT = Axis(Y, -)
TriggerLT = Axis(Y, +)
</pre>
# In ''Options → Controls'', choose the controller, set analogue pedals and bind them by pulling RT/LT (the game's menu shows the pedal axis as "Z axis").
}}
```

Notes: tested only with Xidi 4.3.1 and one pad. The launcher's full INI also
sets 5% stick and 0% trigger deadzones and `ActiveVirtualControllerMask = 0x01`;
the lines above are the part that matters. Choosing the controller in the
game's menu froze the game twice under Wine, although the choice was saved.
Worth a `{{--}}` line if the owner sees it on Windows too.

### C6 — Colin McRae Rally 04: Wine/Proton (PCGW Linux note)

Not eligible for AppDB 2751 (the working setup uses the dgVoodoo/DXVK
wrappers). Turn this into a PCGW "Linux" `{{ii}}`/fixbox, or keep it as
reference text:

> Wine 11.20/11.21 (WoW64). Plays at 4K with dgVoodoo2 **2.80** `D3D9.dll`
> (MS/x86) + DXVK 3.1.1 x32 `d3d11.dll`/`dxgi.dll`; dgVoodoo2 2.87.6
> crashed. A fresh prefix needs `AUDIO_PATH`, `CD_PATH`, `INSTALL_PATH`
> and `VIDEO_PATH` in the game's registry key (32-bit view) pointing at the
> install folder, or the game crashes at start. With a portrait primary
> monitor the picture is a thin strip: set dgVoodoo's `FullScreenOutput`
> to the landscape monitor. Without a wrapper (Xidi), Wine's pad gives
> buttons and hat but no working axes. Alt+F4 does nothing (see above).
> Avoid the in-game Advanced Graphics menu: it rewrote the resolution and
> hung.

Before posting, take the exact registry key path from
`rea-colin-mcrae-4/src/registry.rs`; it is not repeated here. The
Advanced Graphics hang comes from the earlier recipe, not a dated run.
Drop it if the owner cannot reproduce it.

### R1 — Sega Rally Revo: name the stutter fix

Page: <https://www.pcgamingwiki.com/wiki/Sega_Rally_Revo>. The current
"Enable high frame rate" fixbox names no tool, and the only community
workaround elsewhere is a 0.96 Cheat Engine speedhack. Draft:

```wikitext
{{Fixbox|description=Use SegaRallyRevoStutterFix|ref=<ref>{{Refcheck|user=OWNER|date=2026-11-02|comment=Applied to SEGA Rally_SSE1.exe v4.0.6.0; result matches the fix's reference hash; races driven on Wine 11.20 + DXVK.}}</ref>|fix=
{{ii}} Fixes the frame drop/duplicate every ~26 frames (the race clock advances in whole milliseconds, 16/17 ms) and the extra stutter from stage 2 of Championship (a second pacer thread).
{{ii}} Only for the clean {{file|SEGA Rally_SSE1.exe}} (3,727,360 bytes, MD5 <code>4e71704338c6d7f3ef57ea81fe5b015d</code>); launch the game through that file afterwards.
# Download [https://github.com/stuken/SegaRallyRevoStutterFix SegaRallyRevoStutterFix] and follow its instructions.
}}
```

Notes: this is stuken's work. Our part is confirming that it applies to the
widely mirrored v4.0.6.0 SSE1 build (SHA-256
`22655a54bad6932239ade58325703bd4b5a2020376737748026443fa5aadf76e`) and
reproduces the reference result
(`4c08c1deae28a329b6588138198556ac8eaded8444d0eb94a1fe2f1ad08f2520`). In-race
frame pacing was not measured; the owner's race recording is the
gameplay evidence. Also mention, as `{{ii}}` or in the note, that DXVK's
`d3d9.maxFrameRate = 60` is the usual extra guard under Linux.
Martijnsx/Sega-Rally-Revo-Frame-Pacing-Fix targets a different build
(3,700,786 bytes) and should not be suggested for this one.

### R3 — Sega Rally Revo under Wine (PCGW Linux note)

The retail AppDB entry is rated Garbage (Wine 3.0.3). Our verified runs
used DXVK, so they are not AppDB material. A plain-Wine retest (no DXVK) is
needed for an AppDB report. Text for a PCGW Linux note (do not mention the
copy's origin, and describe the disc check only in general terms):

> Wine 11.20 (WoW64) + DXVK 3.1.1 d3d9: plays (races driven with an XInput
> pad). Needed:
> - the June 2010 DirectX runtime pieces `winetricks d3dx9 directplay`
>   (a main-menu access violation otherwise);
> - `Documents\SEGA Rally\Saved Games\config.ini` written by hand, because
>   the .NET `SEGARallyLauncher.exe` is unreliable under Wine;
> - the saved-game layout `Saved Games\SEGA Rally.dat` and
>   `Public Documents\SEGA Rally\Shared Data\Rank Table\SR_RANKTABLES.DAT`,
>   or the main menu crashes;
> - on multi-monitor hosts with a high-refresh monitor, a Wine virtual
>   desktop, because the game ignores `RunWindowed=1` and its exclusive
>   mode change fails.
>
> The startup logo videos (`Bootup/video/Rally/*.wmv`) take very long under
> Wine's GStreamer path, and renaming them skips them. On hybrid
> laptops/desktops DXVK may pick the iGPU and show black frames, so force
> the discrete GPU's Vulkan ICD.

Caveat: the saved-game crash may be specific to installs that never ran
the original installer/launcher. Phrase it as "if …".

### O1 — OutRun 2006: d-pad in menus under Wine/Proton

Page: <https://www.pcgamingwiki.com/wiki/OutRun_2006:_Coast_2_Coast>. Add
to the existing "Linux/SteamOS" area, or as an `{{ii}}` under Input:

```wikitext
{{ii}} With [[#OutRun2006Tweaks|OutRun2006Tweaks]]' new input system under Wine/Proton, the default <code>InputBackend = 0</code> (Windows.Gaming.Input) leaves the D-pad dead in menus (only the stick moves the selection); set <code>InputBackend = 3</code> (XInput) in {{file|OutRun2006Tweaks.ini}}.<ref>{{Refcheck|user=OWNER|date=2026-11-02|comment=Wine 11.21, gamescope, Xbox-style pad, Tweaks build 08e5efb4.}}</ref>
```

---

## Ready drafts: upstream

### O1 upstream — emoose/OutRun2006Tweaks issue

Search existing issues first. #54, #116, #199, #141, #319 and #296 are
related but not this.

> **Title:** Under Wine/Proton, InputBackend = 0 (WGI) ignores the D-pad in menus
>
> Build: master `08e5efb4` (CI run 32972770753), `UseNewInput = true`.
> Wine 11.21 (also expected on Proton), Xbox-style XInput pad.
>
> With `InputBackend = 0` (Windows.Gaming.Input) the D-pad does nothing in
> the menus, and the left stick moves the selection only sometimes. Driving
> works. With `InputBackend = 3` (XInput) the D-pad works in menus.
> `ControllerHotPlug` was off.
>
> Wine's Windows.Gaming.Input is incomplete, so this is likely Wine's side.
> A note in the INI comment or the README's Linux line ("under Wine/Proton
> use InputBackend = 3") would save Linux players the search. Happy to test
> builds.

### X1 — linker issue (optional, not game-related)

Seen in three launchers (OutRun, Sega Rally Revo, CMR3 local builds):
with mingw-w64 GNU ld 2.47 (binutils 2.47.20260726), `windows-sys` 0.61
raw-dylib imports from `kernel32.dll` (`CloseHandle`,
`GetModuleHandleA/W`, `LoadLibraryW`, `Sleep`) end up outside every import
descriptor. The IAT slot is never filled, and the first call jumps to the
hint/name RVA. Linking with lld works. Before filing (to
microsoft/windows-rs or sourceware binutils), build a minimal crate that
reproduces it and check that the bug is not already known. No draft until
then.

---

## After verification

Each item still needs a real-game run before any of it goes on a wiki.
Offsets are file offsets, computed from the LE page map and checked against
the original bytes of the executables listed (hashes in
`rea-screamer/docs/04-…` §1).

### S2 — Hor+ for Screamer 2 / Screamer Rally

These use the same 31-byte routine and original bytes as S1, plus a 5-byte
`call`:

| Executable (SHA-256 prefix) | routine offset | call offset | call bytes |
|---|---|---|---|
| S2 `S2_3DFX.EXE` (`e6c24c84…`) | 08A41E | 0899B2 | `E8 67 0A 00 00` |
| S2 `START65H.EXE` (`05f57686…`) | 09334E | 0928F2 | `E8 57 0A 00 00` |
| S2 `STARTH.EXE` (`91a49f26…`) | 09270E | 091CB2 | `E8 57 0A 00 00` |
| SR `STRT3FX.EXE` (`9eac6e12…`) | 087FEE | 087582 | `E8 67 0A 00 00` |
| SR `START65H.EXE` (`0785e385…`) | 08564E | 084BF2 | `E8 57 0A 00 00` |
| SR `STARTH.EXE` (`18d32c27…`) | 08A4FE | 089AA2 | `E8 57 0A 00 00` |

Needed: an in-race visual check in each game. One reason for caution:
`docs/04-…` §3.2 describes Screamer 2/Rally's camera matrix as
**floating-point**, while the patch applies the same integer `sar 2` routine
as Screamer 1. The patched call site does look identical to Screamer 1's
(`lea esi,[edx+48h]` … `sub eax,eax; mov [esi+24h],eax`), so it may be an
integer copy, but if the matrix there is float the result would be wrong.
Do not publish until it is seen working.

### S6 — F7, 3dfx texture upload at the 2 MB boundary

The bug was reproduced with the original exe: switching track previews
exits with `grTexDownloadMipMap: mipmap cannot span 2 Mbyte boundary`. Fix:

| Executable | offset | original → new |
|---|---|---|
| `S2_3DFX.EXE` | 0D3457 | `8E` → `8C` (`jle` → `jl`) |
| | 0D3658 | `7D 10` → `90 90` |
| | 0D3826 | `7D 10` → `90 90` |
| `STRT3FX.EXE` | 0D0FF7 | `8E` → `8C` |
| | 0D11F8 | `7D 10` → `90 90` |
| | 0D13C6 | `7D 10` → `90 90` |

Needed: repeat the same track-preview sequence with the patched exe and
play a race. The second and third bytes are the 2nd byte of a `0F 8E`/`jge`;
the offsets point exactly at the changed bytes.

### S7 — E5 deadzone (10% centre deadzone, `cmp eax,66h`)

Change `66` → `00` at: S1 `STARTH` 02F73E, S1 `STARTL` 02F6DE, S2 `S2_3DFX`
03FE04, S2 `START65H` 0473B3, S2 `STARTH` 0449DB, SR `STRT3FX` 03F644,
SR `START65H` 03D2B4, SR `STARTH` 03BE84.

`docs/04-…` §5.1 gives 02F73C/03FE02/03F642 because it points at the
start of the `83 F8 66` instruction, two bytes earlier. Needed: a test
with DOSBox's own joystick emulation. The launcher's injection sets
`record.enabled = 2`, which skips this code path entirely, so E5 has never
had an observable effect in our runs.

### S8 — E6 draw distance

- S2/SR: five `cmp eax, 14257880h` (`3D 80 78 25 14` → `3D 00 E2 95 50`) per exe.
- S1: float 40000 → 48000 plus two 52000 → 62400 gates. `STARTH`: 0F7529
  `00 40 1C 47` → `00 80 3B 47`; 02B1BB and 02B1D2 `20 CB 00 00` →
  `C0 F3 00 00`.
- `S2_3DFX`: 081D0F, 081D73, 081DF9, 081E5D, 089652.
- `STRT3FX`: 07F8DF, 07F943, 07F9C9, 07FA2D, 087222.

The other renderers' offsets can be computed the same way from `fixes.rs`.
Needed: a visual before/after in a race. Only a headless Rally race
segment ran.

### S9 — DOSBox compatibility fixes

These are known community pain points: the Screamer 1 SVGA crash (PCGW's
"VGA vs SVGA", track 5) and SETUP crashing at high cycles.

- F3 `STARTH` (S1) 09665F `83 E0 07` → `90 90 90`: WinGranularity mask. It
  is a plausible cause of SVGA bank-switch faults, **but no link to the
  track-5 crash has been shown**.
- F2 `STARTH` (S1) 03A0CA `53 51 52 56 57 55` → `B8 00 00 00 01 C3`: DPMI
  probe returns 16 MB.
- F5 S2 `START65H` 0EC02F `7C` → `EB`: force VESA 0x111. `docs/03-…` wrongly
  says 0EB02F; see below.
- F6 `SETUP.EXE` S1 03384B/0338F2, S2 038553/038606, SR `ST.EXE`
  03B2A3/03B356: `F7 F9` → `90 90`. This NOPs `idiv ecx`, so the benchmark
  result is meaningless; check SETUP's resulting choices.

Needed for each: a before/after run on the real game under the stated
DOSBox settings. For F3, a run of track 5 in SVGA with and without it.

### R2 — Sega Rally Revo ultra-wide, clean SSE1 build

PCGW sends ultra-wide players to Widescreen Fixer, and WSGF's method
searches `39 8E E3 3F`. For `SEGA Rally_SSE1.exe` v4.0.6.0 (SHA-256
`22655a54…a5aadf76e`), the two 16:9 slots are at file offsets **2F7C44**
and **2FADA8** (each `39 8E E3 3F`, flanked by 4:3 `AB AA AA 3F` and 16:10
`CD CC CC 3F`). Replace both with the target ratio: 21:9 (64/27 =
2560x1080) `26 B4 17 40`; 3440x1440 `8E E3 18 40`; 32:9 `39 8E 63 40`.
Needed: a 21:9 test. The reference host is 16:9 and the ultra-wide path
was only unit-tested. Do not post offsets for `SEGA Rally.exe` from our
copy: it is a NoDVD build.

### Unverified Wine items (do not post yet)

- winebus `DisableHidraw`/`DisableInput`/`Enable SDL`/`Map Controllers`
  for Sega Rally Revo. These values were never really set in any verified
  run, because the old spelling had a space.
- `SDL_*_ALLOW_BACKGROUND_EVENTS` for OutRun.

---

## Already documented (no action)

- CMR3: widescreen, FOV, borderless, vsync, the high-FPS menu issue, DRM
  removal (SilentPatch + DRM-free patcher); the XInput/force feedback
  module.
- CMR04: registry widescreen (Vert-), AuToMaNiAk005's widescreen fix +
  dgVoodoo, FOV camera hack.
- Sega Rally Revo: ultra-wide via Widescreen Fixer; 62.5 FPS Championship
  cap; SSE1 exe avoids the disc check.
- OutRun 2006: everything OutRun2006Tweaks provides; the Linux DXVK
  `d3d9=n,b` fix.

## Repo inconsistencies found while drafting (fix in the repos, not online)

- `rea-screamer`: the Glide OVL hashes disagree three ways.
  - `edition.rs` says stub `ad743bca…` and DOS driver `05975df4…`.
  - `docs/RUNTIME_NOTES.md` §4 says `b4df3c2d…` / `65d2a7e5…`.
  - The files on disk hash to `fe6f33d3…` / `6981a7bd…`; the sizes match.

  The launcher selects by size (< 50,000 bytes), so behaviour is unaffected.
- `rea-screamer/docs/03-screamer-binary-analysis.md` §4.2: the F5 file
  offset should be `0x0EC02F` (and the range `0x0EC025..`), not `0x0EB02F`;
  0x0EB02F holds `FF` bytes. `docs/04-…` §2 also gives F2 a different
  function/offset (`0x02F758` / `0x039F58`) from the one `fixes.rs` patches
  (VA `0x02F8CA`, file `0x03A0CA`).
- The task brief's "60/120 FPS via smoothed_dt clamp + SOS timer" does not
  match the shipped code: E1 is an empty patch and the docs explain why.
