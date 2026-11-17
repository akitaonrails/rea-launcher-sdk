//! Pure helpers for finding a game session's processes: the reading of
//! `/proc` and Windows' process lists stays in `rea-launcher-native`
//! (`group`), the decisions are here and tested on any host. From the
//! TeknoParrot hubs (sega's `proc.rs`, wangan's `winproc.rs`).

/// The processes `pid` started, and theirs, from the system's
/// `(process, parent)` list. Windows keeps a dead parent's id on its
/// children and reuses ids, so a process counts only when it was `created`
/// at or after its parent: a program the player started from some
/// long-gone process that had our id is never taken for the game's.
/// `created` is asked only about candidates; `None` (unreadable) never
/// counts, and without `pid`'s own time nothing does.
pub fn descendants(
    processes: &[(u32, u32)],
    pid: u32,
    created: impl Fn(u32) -> Option<u64>,
) -> Vec<u32> {
    descendants_at(processes, pid, created)
        .into_iter()
        .map(|(p, _)| p)
        .collect()
}

/// [`descendants`], each with the creation time it was matched on: end a
/// process only when its handle shows that same time (its id may have
/// been reused since the list was taken).
pub fn descendants_at(
    processes: &[(u32, u32)],
    pid: u32,
    created: impl Fn(u32) -> Option<u64>,
) -> Vec<(u32, u64)> {
    let mut out: Vec<(u32, u64)> = Vec::new();
    let mut next = vec![(pid, created(pid))];
    while let Some((parent, born)) = next.pop() {
        let Some(born) = born else { continue };
        for &(child, of) in processes {
            if of != parent || child == parent || child == pid || out.iter().any(|o| o.0 == child) {
                continue;
            }
            if let Some(at) = created(child).filter(|&c| c >= born) {
                out.push((child, at));
                next.push((child, Some(at)));
            }
        }
    }
    out
}

/// Whether a process environment (`/proc/<pid>/environ`: `NAME=value`
/// entries, each ending in a NUL) holds `var` set to exactly `value`.
pub fn environ_has(environ: &[u8], var: &str, value: &str) -> bool {
    environ.split(|b| *b == 0).any(|kv| {
        kv.len() == var.len() + 1 + value.len()
            && kv.starts_with(var.as_bytes())
            && kv[var.len()] == b'='
            && kv.ends_with(value.as_bytes())
    })
}

/// Whether `/proc/<pid>/stat` describes a process that has ended and
/// waits to be reaped (state `Z`, or `X` while it goes): it runs nothing,
/// and counting or signalling it would be wrong. The state follows the
/// name, which is in parentheses and may itself hold `)` or spaces.
pub fn is_zombie(stat: &str) -> bool {
    stat.rsplit_once(')')
        .and_then(|(_, rest)| rest.trim_start().chars().next())
        .is_some_and(|state| matches!(state, 'Z' | 'X'))
}

/// A process's arguments from `/proc/<pid>/cmdline` (NUL-separated;
/// empty entries are dropped, invalid UTF-8 is replaced).
pub fn cmdline(raw: &[u8]) -> Vec<String> {
    raw.split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect()
}

/// Whether the program `exe` (a full Windows path) lies inside the folder
/// `dir`: case-insensitive, either separator, with or without the `\\?\`
/// prefix. The folder itself, a sibling whose name starts the same and an
/// empty `dir` never match.
pub fn under_dir(exe: &str, dir: &str) -> bool {
    fn norm(s: &str) -> String {
        let s = s.replace('/', "\\");
        let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
        s.trim_end_matches('\\').to_lowercase()
    }
    let (exe, dir) = (norm(exe), norm(dir));
    !dir.is_empty()
        && exe.len() > dir.len() + 1
        && exe.starts_with(&dir)
        && exe[dir.len()..].starts_with('\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The process list and a start-time lookup from (pid, parent, created).
    fn listed(rows: &[(u32, u32, Option<u64>)]) -> (Vec<(u32, u32)>, impl Fn(u32) -> Option<u64>) {
        let times: Vec<(u32, Option<u64>)> = rows.iter().map(|r| (r.0, r.2)).collect();
        (rows.iter().map(|r| (r.0, r.1)).collect(), move |id| {
            times.iter().find(|t| t.0 == id).and_then(|t| t.1)
        })
    }

    #[test]
    fn the_processes_a_game_starts_are_its_descendants() {
        // 340: the master (created at 100); 576: the slave it started;
        // 900: a helper the slave started; 13: another program; 0: Windows'
        // idle process, its own parent
        let (ps, at) = listed(&[
            (0, 0, Some(0)),
            (12, 4, Some(5)),
            (340, 100, Some(100)),
            (576, 340, Some(110)),
            (900, 576, Some(120)),
            (13, 12, Some(6)),
        ]);
        assert_eq!(descendants(&ps, 340, &at), vec![576, 900]);
        assert_eq!(descendants(&ps, 576, &at), vec![900]);
        assert!(descendants(&ps, 900, &at).is_empty());
        assert!(descendants(&ps, 0, &at).is_empty());
        // a loop in stale records doesn't hang
        let (ring, at) = listed(&[(1, 2, Some(1)), (2, 1, Some(1))]);
        assert_eq!(descendants(&ring, 1, &at), vec![2]);
    }

    #[test]
    fn descendants_come_with_the_creation_time_they_were_matched_on() {
        let (ps, at) = listed(&[
            (340, 100, Some(100)),
            (576, 340, Some(110)),
            (900, 576, Some(120)),
        ]);
        // ended by these exact times later, never by "created after"
        assert_eq!(descendants_at(&ps, 340, &at), vec![(576, 110), (900, 120)]);
    }

    #[test]
    fn a_program_whose_dead_parent_had_our_id_is_not_the_games() {
        // the player's browser (700) was started at 50 by a process that
        // had id 340 and has since quit; our master got id 340 at 100
        let (ps, at) = listed(&[
            (340, 100, Some(100)),
            (700, 340, Some(50)),
            (701, 700, Some(60)),
            (576, 340, Some(110)),
            // a process whose start time can't be read: left alone
            (800, 340, None),
        ]);
        assert_eq!(descendants(&ps, 340, &at), vec![576]);
        // without the master's own time nothing counts
        let (ps, at) = listed(&[(340, 100, None), (576, 340, Some(110))]);
        assert!(descendants(&ps, 340, &at).is_empty());
    }

    #[test]
    fn the_session_tag_is_one_whole_environment_entry() {
        let env = b"HOME=/home/p\0SEGA_SESSION=/rt/f355\0PATH=/usr/bin\0";
        assert!(environ_has(env, "SEGA_SESSION", "/rt/f355"));
        assert!(environ_has(env, "HOME", "/home/p"));
        // another session's runtime that starts or ends the same
        assert!(!environ_has(env, "SEGA_SESSION", "/rt/f35"));
        assert!(!environ_has(env, "SEGA_SESSION", "/rt/f3555"));
        assert!(!environ_has(env, "SEGA_SESSION", "rt/f355"));
        // another variable with the value, a variable that starts the same
        assert!(!environ_has(env, "OTHER", "/rt/f355"));
        assert!(!environ_has(env, "SEGA", "SESSION=/rt/f355"));
        assert!(!environ_has(b"", "SEGA_SESSION", ""));
        assert!(environ_has(b"SEGA_SESSION=\0", "SEGA_SESSION", ""));
    }

    #[test]
    fn a_zombie_is_read_after_the_name_in_parentheses() {
        assert!(is_zombie("4242 (gamescope) Z 1 4242 4242 0 -1"));
        assert!(is_zombie("4242 (wine) X 1"));
        assert!(!is_zombie("4242 (gamescope) S 1 4242 4242 0 -1"));
        assert!(!is_zombie("4242 (gamescope) R 1"));
        // a name that holds a parenthesis and a Z of its own
        assert!(!is_zombie("77 (a) Z (b) S 1 77"));
        assert!(is_zombie("77 (a) S (b) Z 1 77"));
        assert!(!is_zombie(""));
        assert!(!is_zombie("garbage"));
    }

    #[test]
    fn a_command_line_splits_at_nuls() {
        assert_eq!(
            cmdline(b"/c/fc/flycast\0-config\0naomi:BoardId=1\0\0"),
            ["/c/fc/flycast", "-config", "naomi:BoardId=1"]
        );
        assert!(cmdline(b"").is_empty());
        assert_eq!(cmdline(b"a\xffb\0"), ["a\u{fffd}b"]);
    }

    #[test]
    fn programs_inside_the_runtime_only() {
        let rt = r"C:\Users\p\AppData\Local\rea-wangan\runtimes\mt6";
        assert!(under_dir(
            r"C:\Users\p\AppData\Local\rea-wangan\runtimes\mt6\AMCUS\AMAuthd.exe",
            rt
        ));
        assert!(under_dir(
            r"c:\users\P\appdata\local\rea-wangan\runtimes\MT6\wangan-io.exe",
            &format!(r"\\?\{rt}\")
        ));
        assert!(under_dir(
            "Z:/mnt/x/runtimes/mt6/wmn6r.exe",
            r"Z:\mnt\x\runtimes\mt6"
        ));
        // a sibling runtime with a longer name, the folder itself, elsewhere
        assert!(!under_dir(
            r"C:\Users\p\AppData\Local\rea-wangan\runtimes\mt6rr\wmn6r.exe",
            rt
        ));
        assert!(!under_dir(rt, rt));
        assert!(!under_dir(r"C:\Windows\explorer.exe", rt));
        assert!(!under_dir(r"C:\Windows\explorer.exe", ""));
    }
}
