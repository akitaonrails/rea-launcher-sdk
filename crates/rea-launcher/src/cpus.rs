//! Keeping a game on alike CPUs: one cache domain, performance cores only.
//!
//! From the Sega hub launcher's `launch.rs` and `session.rs`: Flycast's
//! linked boards (F355 Challenge) often fail to boot when they run on cores
//! of different speeds (flycast#1630; on a Ryzen 9 7950X3D, 1 boot in 6
//! unpinned, 3 in 4 on one CCD). [`uniform_cpus`] picks the set; Linux
//! describes the CPUs in sysfs ([`linux_cpus`], the set goes to `taskset -c`
//! through [`crate::gamescope::CommandLine::cpus`]), Windows in a
//! `GetLogicalProcessorInformationEx` buffer ([`windows_cpus`], the set
//! goes to the process's affinity in `rea-launcher-native`).

use std::fs;
use std::path::Path;

/// A CPU as the system describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cpu {
    pub id: u32,
    /// How fast the system ranks it: Linux `acpi_cppc/highest_perf`, else
    /// `cpufreq/cpuinfo_max_freq`; Windows the core's efficiency class.
    pub rank: u64,
    /// The CPUs sharing its last-level cache.
    pub l3: Vec<u32>,
}

/// Parses a Linux CPU list (`0-7,16-23`).
pub fn cpu_list(text: &str) -> Vec<u32> {
    let mut v = Vec::new();
    for part in text.trim().split(',').filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                    v.extend(a..=b);
                }
            }
            None => v.extend(part.trim().parse::<u32>().ok()),
        }
    }
    v
}

/// The CPUs a game that needs alike cores should share: the cache domain
/// of the CPU the system ranks fastest, and on a hybrid CPU only its
/// performance cores (`core`). None when every CPU is alike or fewer than
/// four remain.
pub fn uniform_cpus(cpus: &[Cpu], core: Option<&[u32]>) -> Option<Vec<u32>> {
    let best = cpus
        .iter()
        .max_by_key(|c| (c.rank, std::cmp::Reverse(c.id)))?;
    let mut set: Vec<u32> = cpus
        .iter()
        .map(|c| c.id)
        .filter(|id| best.l3.contains(id))
        .filter(|id| core.is_none_or(|core| core.contains(id)))
        .collect();
    set.sort_unstable();
    (set.len() >= 4 && set.len() < cpus.len()).then_some(set)
}

/// The CPUs as Windows describes them: the buffer
/// `GetLogicalProcessorInformationEx(RelationAll)` fills, read for
/// processor group 0 (`Cpu::rank` is the core's efficiency class). The
/// second value is the performance cores of a hybrid CPU, for
/// [`uniform_cpus`]'s `core`; None when every core is alike.
pub fn windows_cpus(buf: &[u8]) -> (Vec<Cpu>, Option<Vec<u32>>) {
    const CORE: u32 = 0; // RelationProcessorCore
    const CACHE: u32 = 2; // RelationCache
    let u16_at = |i: usize| buf.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |i: usize| {
        buf.get(i..i + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let u64_at = |i: usize| {
        buf.get(i..i + 8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    };
    let bits = |mask: u64| (0..64).filter(move |b| mask & (1 << b) != 0);
    // (efficiency class, CPUs) per core; the CPUs of each level-3 cache
    let mut cores: Vec<(u8, u64)> = Vec::new();
    let mut l3s: Vec<u64> = Vec::new();
    let mut at = 0;
    while let (Some(kind), Some(size)) = (u32_at(at), u32_at(at + 4)) {
        let size = size as usize;
        if size == 0 {
            break;
        }
        match kind {
            // PROCESSOR_RELATIONSHIP: EfficiencyClass at 9, GroupMask at 32
            CORE if u16_at(at + 40) == Some(0) => {
                if let (Some(class), Some(mask)) = (buf.get(at + 9), u64_at(at + 32)) {
                    cores.push((*class, mask));
                }
            }
            // CACHE_RELATIONSHIP: Level at 8, GroupMask at 40
            CACHE if buf.get(at + 8) == Some(&3) && u16_at(at + 48) == Some(0) => {
                l3s.extend(u64_at(at + 40));
            }
            _ => {}
        }
        at += size;
    }
    let mut cpus: Vec<Cpu> = cores
        .iter()
        .flat_map(|&(class, mask)| {
            let l3 = l3s
                .iter()
                .copied()
                .find(|l3| l3 & mask != 0)
                .unwrap_or(mask);
            bits(mask).map(move |id| Cpu {
                id,
                rank: class as u64,
                l3: bits(l3).collect(),
            })
        })
        .collect();
    cpus.sort_by_key(|c| c.id);
    let top = cpus.iter().map(|c| c.rank).max();
    let core = (cpus.iter().any(|c| Some(c.rank) != top)).then(|| {
        cpus.iter()
            .filter(|c| Some(c.rank) == top)
            .map(|c| c.id)
            .collect()
    });
    (cpus, core)
}

/// The CPUs as Linux describes them under `sys` (`/sys` on a real
/// system), and the performance cores of a hybrid Intel CPU
/// (`devices/cpu_core/cpus`), for [`uniform_cpus`]. Empty when `sys` says
/// nothing.
pub fn linux_cpus(sys: &Path) -> (Vec<Cpu>, Option<Vec<u32>>) {
    let base = sys.join("devices/system/cpu");
    let read = |p: &Path| fs::read_to_string(p).ok();
    let online = read(&base.join("online")).map_or_else(Vec::new, |t| cpu_list(&t));
    let cpus = online
        .into_iter()
        .map(|id| {
            let dir = base.join(format!("cpu{id}"));
            let rank = read(&dir.join("acpi_cppc/highest_perf"))
                .or_else(|| read(&dir.join("cpufreq/cpuinfo_max_freq")))
                .and_then(|t| t.trim().parse().ok())
                .unwrap_or(0);
            let l3 = read(&dir.join("cache/index3/shared_cpu_list"))
                .map(|t| cpu_list(&t))
                .unwrap_or_default();
            Cpu { id, rank, l3 }
        })
        .collect();
    let core = read(&sys.join("devices/cpu_core/cpus")).map(|t| cpu_list(&t));
    (cpus, core)
}

/// A CPU set as `taskset -c` takes it (`8,9,10,11`).
pub fn taskset_list(cpus: &[u32]) -> String {
    cpus.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `GetLogicalProcessorInformationEx` buffer: cores as (efficiency
    /// class, CPU mask), level-3 caches as CPU masks.
    fn slpi(cores: &[(u8, u64)], l3s: &[u64]) -> Vec<u8> {
        let mut b = Vec::new();
        for &(class, mask) in cores {
            let mut r = vec![0u8; 48];
            r[4..8].copy_from_slice(&48u32.to_le_bytes());
            r[9] = class;
            r[30..32].copy_from_slice(&1u16.to_le_bytes());
            r[32..40].copy_from_slice(&mask.to_le_bytes());
            b.extend(r);
        }
        for &mask in l3s {
            // a level-1 cache first, which doesn't count
            for level in [1u8, 3] {
                let mut r = vec![0u8; 56];
                r[0..4].copy_from_slice(&2u32.to_le_bytes());
                r[4..8].copy_from_slice(&56u32.to_le_bytes());
                r[8] = level;
                r[38..40].copy_from_slice(&1u16.to_le_bytes());
                r[40..48].copy_from_slice(&mask.to_le_bytes());
                b.extend(r);
            }
        }
        // a NUMA node record, skipped
        let mut r = vec![0u8; 80];
        r[0..4].copy_from_slice(&1u32.to_le_bytes());
        r[4..8].copy_from_slice(&80u32.to_le_bytes());
        b.extend(r);
        b
    }

    #[test]
    fn windows_cpu_records_pick_one_cache_domain() {
        // two CCDs of 4 cores with SMT (CPUs 0-7 and 8-15), all one class:
        // the first CCD
        let cores: Vec<(u8, u64)> = (0..8).map(|c| (0, 0b11u64 << (2 * c))).collect();
        let (cpus, core) = windows_cpus(&slpi(&cores, &[0x00ff, 0xff00]));
        assert_eq!(cpus.len(), 16);
        assert_eq!(cpus[9].l3, (8..16).collect::<Vec<u32>>());
        assert_eq!(core, None);
        assert_eq!(uniform_cpus(&cpus, core.as_deref()), Some((0..8).collect()));
        // hybrid: 4 performance cores with SMT (0-7), 8 efficiency cores
        // (8-15), one shared L3: the performance cores
        let mut cores: Vec<(u8, u64)> = (0..4).map(|c| (1, 0b11u64 << (2 * c))).collect();
        cores.extend((8..16).map(|c| (0, 1u64 << c)));
        let (cpus, core) = windows_cpus(&slpi(&cores, &[0xffff]));
        assert_eq!(core.as_deref(), Some(&(0..8).collect::<Vec<u32>>()[..]));
        assert_eq!(uniform_cpus(&cpus, core.as_deref()), Some((0..8).collect()));
        // one CCD, all alike: nothing to pin; a short or empty buffer: nothing
        let cores: Vec<(u8, u64)> = (0..4).map(|c| (0, 0b11u64 << (2 * c))).collect();
        let (cpus, core) = windows_cpus(&slpi(&cores, &[0xff]));
        assert_eq!(uniform_cpus(&cpus, core.as_deref()), None);
        assert!(windows_cpus(&[1, 2, 3]).0.is_empty());
        assert!(windows_cpus(&[]).0.is_empty());
    }

    fn cpu(id: u32, rank: u64, l3: &str) -> Cpu {
        Cpu {
            id,
            rank,
            l3: cpu_list(l3),
        }
    }

    #[test]
    fn cpu_lists_parse_like_linux_writes_them() {
        assert_eq!(cpu_list("0-3,8,10-11\n"), vec![0, 1, 2, 3, 8, 10, 11]);
        assert_eq!(cpu_list(""), Vec::<u32>::new());
        assert_eq!(cpu_list("x,2"), vec![2]);
    }

    #[test]
    fn the_boards_share_the_fastest_cache_domain() {
        // a Ryzen 9 7950X3D: two CCDs, the faster one ranked higher
        let mut cpus = Vec::new();
        for id in 0..32u32 {
            let ccd1 = (8..16).contains(&(id % 16));
            let rank = if ccd1 {
                206 + (id % 8) as u64 * 4
            } else {
                166 + (id % 8) as u64 * 5
            };
            cpus.push(cpu(id, rank, if ccd1 { "8-15,24-31" } else { "0-7,16-23" }));
        }
        assert_eq!(uniform_cpus(&cpus, None), Some(cpu_list("8-15,24-31")));
        // a hybrid Intel CPU: one shared cache, only the performance cores
        let hybrid: Vec<Cpu> = (0..12)
            .map(|id| cpu(id, if id < 8 { 70 } else { 40 }, "0-11"))
            .collect();
        assert_eq!(
            uniform_cpus(&hybrid, Some(&cpu_list("0-7"))),
            Some(cpu_list("0-7"))
        );
        // alike cores: nothing to pin
        let plain: Vec<Cpu> = (0..8).map(|id| cpu(id, 100, "0-7")).collect();
        assert_eq!(uniform_cpus(&plain, None), None);
        // too few left
        let small: Vec<Cpu> = (0..4)
            .map(|id| cpu(id, 100, if id < 2 { "0-1" } else { "2-3" }))
            .collect();
        assert_eq!(uniform_cpus(&small, None), None);
        assert_eq!(uniform_cpus(&[], None), None);
    }

    #[test]
    fn linux_describes_its_cpus_in_sysfs() {
        let sys = crate::test_dir("cpus-sysfs");
        let cpu = sys.join("devices/system/cpu");
        let put = |rel: &str, text: &str| {
            let p = cpu.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, text).unwrap();
        };
        put("online", "0-2\n");
        put("cpu0/acpi_cppc/highest_perf", "206\n");
        put("cpu0/cache/index3/shared_cpu_list", "0-1\n");
        put("cpu1/cpufreq/cpuinfo_max_freq", "4200000\n");
        put("cpu2/acpi_cppc/highest_perf", "junk");
        let (cpus, core) = linux_cpus(&sys);
        assert_eq!(cpus.len(), 3);
        assert_eq!((cpus[0].rank, cpus[0].l3.clone()), (206, vec![0, 1]));
        assert_eq!((cpus[1].rank, cpus[1].l3.clone()), (4_200_000, vec![]));
        assert_eq!(cpus[2].rank, 0);
        assert_eq!(core, None);
        fs::create_dir_all(sys.join("devices/cpu_core")).unwrap();
        fs::write(sys.join("devices/cpu_core/cpus"), "0-1").unwrap();
        assert_eq!(linux_cpus(&sys).1, Some(vec![0, 1]));
        assert_eq!(linux_cpus(&sys.join("none")), (vec![], None));
        fs::remove_dir_all(sys).unwrap();
        assert_eq!(taskset_list(&[8, 9, 10]), "8,9,10");
    }
}
