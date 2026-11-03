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
2. Start the game with the configuration and install the hook:

```rust
use rea_launcher_win32::process::Launch;

let game = Launch::new(&exe)
    .cwd(&runtime)
    .env(rea_close_hook::ENV, "class=PC_CMR4;strategy=terminate")
    .spawn()?;
let _hook = match game.message_hook(&launcher_dir.join("rea_close_hook.dll"), c"close_hook") {
    Ok(hook) => Some(hook),
    Err(error) => { log(&format!("warning: {error}")); None }
};
let code = game.wait()?;
```

Keep the hook alive until `wait` returns. As a last resort the launcher
can still call `Running::close_gracefully`.

CMR4 can move to this DLL with `class=PC_CMR4;strategy=terminate` (its own
behaviour), dropping its `close-hook` crate.

## Tests

The pure parts (request matching, configuration, key names) are tested on
the host. `tests/hook.rs` runs every strategy under Wine
(`scripts/wine-test -p rea-launcher-close-hook`) against
`examples/hooked_game.rs`, a hidden window that ignores close requests like
CMR04; headless Wine needs its null graphics driver
(`HKCU\Software\Wine\Drivers\Graphics = null`), which the tests select in
the test prefix.

## Migration

### v0.4.0

- `close_on = alt_f4_state` is new and off by default: existing
  configurations behave as before. A game whose Alt+F4 does nothing while
  `SC_CLOSE` works (CMR2005: `class=PC_CMR5;strategy=terminate`) adds it:
  `close_on=sc_close,alt_f4,alt_f4_state`. Verify in the real game: Alt+F4
  ends it, and Alt+F4 in another window while the game runs doesn't.
- `Requests` has a new field, `alt_f4_state`: code that builds a
  `Requests { … }` literal adds it (or uses `..Requests::default()`);
  configurations parsed with `Config::parse` need nothing.
