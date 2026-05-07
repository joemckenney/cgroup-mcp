use cgroup_mcp::collector::stats::{
    parse_cpu_stat, parse_io_stat, parse_memory_current, parse_memory_events, parse_memory_stat,
    read_cpu_stat, read_io_stat, read_memory_current, read_memory_events, read_memory_stat,
    CpuStat, MemoryEvents,
};
use std::fs;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stats")
        .join(name);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("reading fixture {}: {e}", p.display()))
}

fn real_arch_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/real_arch")
}

// ---- cpu.stat ----

#[test]
fn parses_cpu_stat_normal_ignoring_unknown_dotted_keys() {
    // Real captures contain `core_sched.force_idle_usec` — verify the parser
    // tolerates dotted keys (as keys, not as separators) and ignores unknown ones.
    let cpu = parse_cpu_stat(&fixture("cpu_stat_normal")).unwrap();
    assert_eq!(cpu.usage_usec, 5126584);
    assert_eq!(cpu.user_usec, 3405331);
    assert_eq!(cpu.system_usec, 1721253);
    assert_eq!(cpu.nr_periods, 0);
}

#[test]
fn parses_cpu_stat_with_throttling() {
    let cpu = parse_cpu_stat(&fixture("cpu_stat_with_throttling")).unwrap();
    assert_eq!(cpu.nr_periods, 100);
    assert_eq!(cpu.nr_throttled, 12);
    assert_eq!(cpu.throttled_usec, 8765);
}

#[test]
fn cpu_stat_missing_fields_default_to_zero() {
    // Older kernels / minimal accounting: only the basic 3 fields present.
    let cpu = parse_cpu_stat(&fixture("cpu_stat_minimal")).unwrap();
    assert_eq!(cpu.usage_usec, 100);
    assert_eq!(cpu.nice_usec, 0);
    assert_eq!(cpu.nr_throttled, 0);
}

#[test]
fn cpu_stat_rejects_non_numeric_value() {
    let bad = "usage_usec twelve\nuser_usec 5\n";
    let err = parse_cpu_stat(bad).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("usage_usec"), "error was: {msg}");
}

// ---- memory.current ----

#[test]
fn parses_memory_current_with_trailing_newline() {
    let n = parse_memory_current("6144000\n").unwrap();
    assert_eq!(n, 6144000);
}

#[test]
fn parses_memory_current_no_newline() {
    assert_eq!(parse_memory_current("0").unwrap(), 0);
}

#[test]
fn rejects_non_numeric_memory_current() {
    let err = parse_memory_current("max\n").unwrap_err();
    assert!(format!("{err:#}").contains("memory.current"));
}

// ---- memory.stat ----

#[test]
fn parses_memory_stat_into_raw_map_with_get_accessor() {
    let m = parse_memory_stat(&fixture("memory_stat_small")).unwrap();
    assert_eq!(m.get("anon"), 1048576);
    assert_eq!(m.get("file"), 524288);
    assert_eq!(m.get("pgmajfault"), 2);
    // unknown key -> 0
    assert_eq!(m.get("not_a_real_field"), 0);
    // raw map preserves all entries
    assert_eq!(m.raw.len(), 6);
}

// ---- memory.events ----

#[test]
fn parses_memory_events_complete() {
    let e = parse_memory_events(&fixture("memory_events_complete")).unwrap();
    assert_eq!(
        e,
        MemoryEvents {
            low: 0,
            high: 5,
            max: 0,
            oom: 2,
            oom_kill: 1,
            oom_group_kill: 0,
        }
    );
}

#[test]
fn memory_events_missing_oom_group_kill_defaults_to_zero() {
    // older kernel without oom_group_kill — must not error
    let e = parse_memory_events(&fixture("memory_events_legacy")).unwrap();
    assert_eq!(e.oom_group_kill, 0);
    assert_eq!(e.oom_kill, 0);
}

// ---- io.stat ----

#[test]
fn parses_io_stat_with_multiple_devices() {
    let v = parse_io_stat(&fixture("io_stat_two_devices")).unwrap();
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].major, 259);
    assert_eq!(v[0].minor, 0);
    assert_eq!(v[0].rbytes, 18329600);
    assert_eq!(v[0].wios, 248257);
    assert_eq!(v[1].major, 8);
    assert_eq!(v[1].rbytes, 2459017728);
}

#[test]
fn empty_io_stat_returns_empty_vec() {
    // A cgroup with no IO activity yet may have an effectively-empty io.stat.
    let v = parse_io_stat(&fixture("io_stat_empty")).unwrap();
    assert!(v.is_empty());
    // also explicit empty string
    assert!(parse_io_stat("").unwrap().is_empty());
}

#[test]
fn io_stat_rejects_malformed_device_prefix() {
    let bad = "no_colon rbytes=10\n";
    let err = parse_io_stat(bad).unwrap_err();
    assert!(format!("{err:#}").contains("major:minor"));
}

#[test]
fn io_stat_rejects_malformed_kv() {
    let bad = "8:0 rbytes_no_equals 10\n";
    let err = parse_io_stat(bad).unwrap_err();
    assert!(format!("{err:#}").contains("key=value"));
}

#[test]
fn io_stat_tolerates_unknown_kv_fields() {
    // forward-compat: kernel may add new io counters
    let s = "8:0 rbytes=10 wbytes=20 future_field=999\n";
    let v = parse_io_stat(s).unwrap();
    assert_eq!(v[0].rbytes, 10);
    assert_eq!(v[0].wbytes, 20);
}

// ---- read_*_from_path ----

#[test]
fn read_functions_work_against_a_path() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stats");
    assert_eq!(read_memory_current(&path.join("memory_current")).unwrap(), 6144000);
    let cpu = read_cpu_stat(&path.join("cpu_stat_normal")).unwrap();
    assert_eq!(cpu.usage_usec, 5126584);
    let _ = read_memory_stat(&path.join("memory_stat_small")).unwrap();
    let _ = read_memory_events(&path.join("memory_events_complete")).unwrap();
    let _ = read_io_stat(&path.join("io_stat_two_devices")).unwrap();
}

// ---- real_arch smoke test: every captured stats file must parse cleanly ----

#[test]
fn parses_every_stats_file_in_real_arch() {
    let root = real_arch_root();
    let mut counts = (0usize, 0usize, 0usize, 0usize, 0usize); // cpu, mem_curr, mem_stat, mem_evt, io
    walk(&root, &mut |p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        match name {
            "cpu.stat" => {
                let _: CpuStat = read_cpu_stat(p).unwrap_or_else(|e| {
                    panic!("parsing {}: {e:#}", p.display())
                });
                counts.0 += 1;
            }
            "memory.current" => {
                let _: u64 = read_memory_current(p).unwrap_or_else(|e| {
                    panic!("parsing {}: {e:#}", p.display())
                });
                counts.1 += 1;
            }
            "memory.stat" => {
                let _ = read_memory_stat(p).unwrap_or_else(|e| {
                    panic!("parsing {}: {e:#}", p.display())
                });
                counts.2 += 1;
            }
            "memory.events" => {
                let _ = read_memory_events(p).unwrap_or_else(|e| {
                    panic!("parsing {}: {e:#}", p.display())
                });
                counts.3 += 1;
            }
            "io.stat" => {
                let _ = read_io_stat(p).unwrap_or_else(|e| {
                    panic!("parsing {}: {e:#}", p.display())
                });
                counts.4 += 1;
            }
            _ => {}
        }
    });
    // sanity: we captured >=1 of each from real_arch (otherwise the smoke test
    // is silently a no-op)
    assert!(counts.0 > 0, "no cpu.stat files seen in real_arch");
    assert!(counts.1 > 0, "no memory.current files seen in real_arch");
    assert!(counts.2 > 0, "no memory.stat files seen in real_arch");
    assert!(counts.3 > 0, "no memory.events files seen in real_arch");
    assert!(counts.4 > 0, "no io.stat files seen in real_arch");
}

fn walk(dir: &Path, f: &mut dyn FnMut(&Path)) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk(&p, f);
        } else {
            f(&p);
        }
    }
}
