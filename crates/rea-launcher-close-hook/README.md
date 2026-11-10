# rea-launcher-close-hook

A DLL that makes an old game honour Alt+F4 and window-close requests
(docs/STANDARDS.md, "Clean exit, single instance"). It is CMR4's close hook
made generic: the launcher installs the DLL's `close_hook` export as a
`WH_GETMESSAGE` hook on the game's main thread, Windows loads the DLL into
the game, and a close request for the game's window becomes the game's
quit path. The request itself is swallowed.

## Configure it (no recompiling)

From the `REA_CLOSE_HOOK` environment variable on the game process, or
else `rea_close_hook.ini` next to the DLL; same keys, `;`-separated in the
variable, one per line (with `#` comments) in the file:

| Key | Values | Default |
|---|---|---|
| `class` | the game window's class (case-insensitive) | any window of the game |
| `close_on` | `sc_close`, `alt_f4`, `wm_close`, `alt_f4_state` | `sc_close, alt_f4` (`wm_close` off: CMR4 closes its movie window with it; `alt_f4_state` off) |
| `strategy` | `terminate` (TerminateProcess), `exit` (ExitProcess: exit handlers and DLL detach run), `quit` (PostQuitMessage), `post_quit` (WM_QUIT to the window), `keys: escape, 300, down, return` (SendInput, for DirectInput games), `post_keys: …` (WM_KEYDOWN/UP to the window) | `terminate` |
| `code` | exit / quit code | 0 |

**`alt_f4_state`: games with an exclusive DirectInput keyboard.** Such a
game (CMR2005) takes the keyboard for itself, so Alt+F4 never reaches its
window as a message and `alt_f4` can't see it. With `alt_f4_state` the hook
reads the keyboard's state (`GetAsyncKeyState` of Alt and F4) on every
message it sees from the game's loop, and acts on a new press of the chord
while the game's window (this process's, of `class`) is in front; Alt+F4
pressed in another program never counts, and a held chord counts once.
Use it next to the others: `close_on = sc_close, alt_f4, alt_f4_state`.
Tested under Wine (`tests/hook.rs`); on Windows record the real-game check
in the launcher's `RUNTIME_NOTES.md`.

In a key sequence, names are `escape`, `return`, `space`, `tab`,
`up`/`down`/`left`/`right`, `f1`–`f12`, a letter or digit, or `0xNN`;
numbers are pauses in milliseconds. A configuration that doesn't parse
leaves the game alone; check it in the launcher with the same parser
(`rea_close_hook::Config::parse`, the crate is also an rlib).

## Ship and enable it

1. Build it for the game's architecture (`cargo build --release -p
   rea-launcher-close-hook --target i686-pc-windows-gnu`) and ship
   `rea_close_hook.dll` next to the launcher (add it to the package
   allowlist).
2. Put the DLL in the game's import table: write the runtime copy of the
   game's executable through `rea_close_hook::import_into` (only the
   runtime copy, never the player's file), copy `rea_close_hook.dll` next
   to it, and start the game with the configuration and
   `REA_CLOSE_HOOK_ATTACH=1`. Windows loads the DLL with the game, and the
   DLL installs the hook on the game's window thread itself:

```rust
use rea_launcher_win32::process::Launch;

let exe = runtime.join("cmr4.exe");
let bytes = std::fs::read(&exe)?;
std::fs::write(&exe, rea_close_hook::import_into(&bytes)?)?;
std::fs::copy(launcher_dir.join(rea_close_hook::DLL), runtime.join(rea_close_hook::DLL))?;
let code = Launch::new(&exe)
    .cwd(&runtime)
    .env(rea_close_hook::ENV, "class=PC_CMR4;strategy=terminate")
    .env(rea_close_hook::ATTACH_ENV, "1")
    .run()?;
```

Nothing crosses processes: v0.4.2 and earlier installed the hook from the
launcher (`Running::message_hook`, `SetWindowsHookExW` on the game's
thread), which antivirus heuristics take for a keylogger. `message_hook`
still works for tests; `launcher-check` (rule `remote-injection`) refuses
it in launcher code from v0.4.3. As a last resort the launcher can still
call `Running::close_gracefully`.

CMR4 can move to this DLL with `class=PC_CMR4;strategy=terminate` (its own
behaviour), dropping its `close-hook` crate.

## Registry redirect (`REA_REGISTRY`)

A game that reads its folders from `HKLM` can't be given them reliably by
an unelevated launcher: its `HKLM` writes land in the HKCU VirtualStore,
and the game sees that copy only while UAC registry virtualization is on
for the game process. Windows doesn't promise that (off by policy, per
key, and always off for an executable with a `requestedExecutionLevel`
manifest, such as Sega Rally Revo's). CMR1 crashed on a player's Windows
11 that way (rea-colin-mcrae#6). So the launcher writes the game's values
to `HKCU`, and this DLL, loaded through the game's import table, sends
the game's opens of its `HKLM` key there.

Set the rules in `REA_REGISTRY` on the game process: `;`- or
newline-separated `FROM -> TO` (roots `HKLM`, `HKCU` or `HKEY_…`):

```text
HKLM\SOFTWARE\Codemasters\Colin McRae Rally 04 -> HKCU\Software\Codemasters\Colin McRae Rally 04
walk HKLM\SOFTWARE\Codemasters\Rally\1.00 -> HKCU\Software\Codemasters\Rally\1.00
```

- A plain rule matches an open of `FROM` or a key below it (whole
  components, any case) and opens the same place under `TO`. That covers
  games that open the full path at once: CMR4 and CMR2005
  (`RegCreateKeyExA` with full access, then reads and writes), SRR
  (`RegOpenKeyExA`).
- `walk` is for games that open `SOFTWARE` from `HKLM` and go down one
  level at a time (CMR1): ancestors of `FROM` on its path open the matching
  ancestors of `TO`, and the hook remembers those handles; a step from one
  of them off `FROM`'s path (`SOFTWARE` → `Microsoft`) opens the original
  `HKLM` key. Values read directly on a walked ancestor (`HKLM\SOFTWARE`
  itself) come from `TO`'s. Both sides of a `walk` rule have the same depth.

The DLL wraps the main executable's imports of `RegOpenKey[Ex]A/W`,
`RegCreateKey[Ex]A/W` and `RegCloseKey`, found by name, when it loads
(before the game's code runs). Unmatched opens go to the function the
import held, untouched; values are read and written through the handle
the game got, so `RegSetValueEx` through a redirected handle lands in
`HKCU`. Other DLLs in the game keep their own registry calls. Rules that
don't parse leave the game alone: check them in the launcher.

```rust
use rea_close_hook::registry::{self, Config};

let rules = Config::parse(
    r"HKLM\SOFTWARE\Codemasters\Colin McRae Rally 04 -> HKCU\Software\Codemasters\Colin McRae Rally 04",
)?;
// The runtime copy imports the DLL (`import_into`, as for the close hook).
Launch::new(&exe)
    .env(registry::ENV, rules.env_value())
    .env(rea_close_hook::ENV, "class=PC_CMR4;strategy=terminate")
    .env(rea_close_hook::ATTACH_ENV, "1")
    .run()?;
```

The launcher's side (write `HKCU`, read it back, `--check`, carry the
game's old values over) is in `docs/playbooks/new-launcher.md`, "Registry".
`tests/registry.rs` runs `examples/registry_game.rs` under Wine with the
DLL in its import table: matched reads come from `HKCU`, other keys stay
`HKLM`'s, writes land in `HKCU`, a CMR1-style walk works, and without the
variable (or with rules that don't parse) nothing changes.

## Tests

The pure parts (request matching, configuration, key names) are tested on
the host. `tests/hook.rs` runs every strategy under Wine
(`scripts/wine-test -p rea-launcher-close-hook`) against
`examples/hooked_game.rs`, a hidden window that ignores close requests like
CMR04; headless Wine needs its null graphics driver
(`HKCU\Software\Wine\Drivers\Graphics = null`), which the tests select in
the test prefix.

## Migration

### v0.6.0

- `REA_REGISTRY` is new and off by default: without the variable the DLL
  does what it did. Launchers that relied on
  `rea_launcher_win32::reg::Key::machine_or_virtual_store` (CMR2005, CMR4,
  SRR) move to it; see the SDK CHANGELOG's v0.6.0 Migration.

### v0.4.0

- `close_on = alt_f4_state` is new and off by default: existing
  configurations behave as before. A game whose Alt+F4 does nothing while
  `SC_CLOSE` works (CMR2005: `class=PC_CMR5;strategy=terminate`) adds it:
  `close_on=sc_close,alt_f4,alt_f4_state`. Verify in the real game: Alt+F4
  ends it, and Alt+F4 in another window while the game runs doesn't.
- `Requests` has a new field, `alt_f4_state`: code that builds a
  `Requests { … }` literal adds it (or uses `..Requests::default()`);
  configurations parsed with `Config::parse` need nothing.
