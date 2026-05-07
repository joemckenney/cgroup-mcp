use cgroup_mcp::collector::resource_stats::read_resource_stats;
use std::fs;
use std::path::{Path, PathBuf};

fn real_arch_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/real_arch")
}

const CPU_STAT_SAMPLE: &str = "usage_usec 100\nuser_usec 60\nsystem_usec 40\n";
const MEM_CURRENT_SAMPLE: &str = "1048576\n";
const MEM_STAT_SAMPLE: &str = "anon 1024\nfile 2048\n";
const MEM_EVENTS_SAMPLE: &str =
    "low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\noom_group_kill 0\n";
const PSI_SAMPLE: &str =
    "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
const IO_STAT_SAMPLE: &str = "8:0 rbytes=100 wbytes=200 rios=10 wios=20 dbytes=0 dios=0\n";

fn write_all_files(dir: &Path) {
    fs::write(dir.join("cpu.stat"), CPU_STAT_SAMPLE).unwrap();
    fs::write(dir.join("cpu.pressure"), PSI_SAMPLE).unwrap();
    fs::write(dir.join("memory.current"), MEM_CURRENT_SAMPLE).unwrap();
    fs::write(dir.join("memory.stat"), MEM_STAT_SAMPLE).unwrap();
    fs::write(dir.join("memory.events"), MEM_EVENTS_SAMPLE).unwrap();
    fs::write(dir.join("memory.pressure"), PSI_SAMPLE).unwrap();
    fs::write(dir.join("io.stat"), IO_STAT_SAMPLE).unwrap();
    fs::write(dir.join("io.pressure"), PSI_SAMPLE).unwrap();
}

#[test]
fn reads_complete_bundle_when_all_files_present() {
    let dir = tempfile::tempdir().unwrap();
    write_all_files(dir.path());
    let r = read_resource_stats(dir.path()).unwrap();
    assert!(r.cpu_stat.is_some());
    assert!(r.cpu_pressure.is_some());
    assert!(r.memory_current.is_some());
    assert!(r.memory_stat.is_some());
    assert!(r.memory_events.is_some());
    assert!(r.memory_pressure.is_some());
    assert!(r.io_stat.is_some());
    assert!(r.io_pressure.is_some());

    assert_eq!(r.memory_current, Some(1_048_576));
    assert_eq!(r.cpu_stat.unwrap().usage_usec, 100);
    assert_eq!(r.io_stat.unwrap().len(), 1);
}

#[test]
fn missing_files_yield_none_per_field() {
    let dir = tempfile::tempdir().unwrap();
    // Only cpu.stat and memory.pressure present — simulates a cgroup with
    // partial controller enablement.
    fs::write(dir.path().join("cpu.stat"), CPU_STAT_SAMPLE).unwrap();
    fs::write(dir.path().join("memory.pressure"), PSI_SAMPLE).unwrap();

    let r = read_resource_stats(dir.path()).unwrap();
    assert!(r.cpu_stat.is_some());
    assert!(r.memory_pressure.is_some());
    assert!(r.cpu_pressure.is_none());
    assert!(r.memory_current.is_none());
    assert!(r.memory_stat.is_none());
    assert!(r.memory_events.is_none());
    assert!(r.io_stat.is_none());
    assert!(r.io_pressure.is_none());
}

#[test]
fn empty_directory_yields_all_none_without_error() {
    // A cgroup with no stat files at all (e.g. accounting fully disabled)
    // should produce an all-None bundle, not an error.
    let dir = tempfile::tempdir().unwrap();
    let r = read_resource_stats(dir.path()).unwrap();
    assert!(r.cpu_stat.is_none());
    assert!(r.cpu_pressure.is_none());
    assert!(r.memory_current.is_none());
    assert!(r.memory_stat.is_none());
    assert!(r.memory_events.is_none());
    assert!(r.memory_pressure.is_none());
    assert!(r.io_stat.is_none());
    assert!(r.io_pressure.is_none());
}

#[test]
fn parse_errors_propagate_rather_than_being_swallowed_as_none() {
    // A *present but malformed* file is a real bug — must not silently
    // become None. The whole call fails so the caller sees the problem.
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("memory.current"), "not_a_number\n").unwrap();
    let err = read_resource_stats(dir.path()).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("memory.current"), "error was: {msg}");
}

#[test]
fn reads_every_captured_real_arch_leaf_service() {
    // Each captured leaf service in real_arch should produce a bundle where
    // cpu_stat, memory_current, memory_stat, memory_events, and all three
    // pressures are populated. (We didn't capture cgroup.procs, but that's
    // not part of ResourceStats.)
    let leaves = [
        "system.slice/dbus-broker.service",
        "system.slice/systemd-journald.service",
        "system.slice/NetworkManager.service",
    ];
    for leaf in leaves {
        let path = real_arch_root().join(leaf);
        let r = read_resource_stats(&path)
            .unwrap_or_else(|e| panic!("reading {leaf}: {e:#}"));
        assert!(r.cpu_stat.is_some(), "{leaf}: cpu_stat");
        assert!(r.memory_current.is_some(), "{leaf}: memory_current");
        assert!(r.memory_stat.is_some(), "{leaf}: memory_stat");
        assert!(r.memory_events.is_some(), "{leaf}: memory_events");
        assert!(r.memory_pressure.is_some(), "{leaf}: memory_pressure");
        assert!(r.cpu_pressure.is_some(), "{leaf}: cpu_pressure");
        assert!(r.io_pressure.is_some(), "{leaf}: io_pressure");
        assert!(r.io_stat.is_some(), "{leaf}: io_stat");
    }
}

#[test]
fn reads_real_arch_root_cgroup() {
    // Root cgroup has a different file set than leaves — notably no
    // memory.current, memory.events, or cgroup.events files were captured
    // there. Bundle must still succeed, with those fields as None.
    let r = read_resource_stats(&real_arch_root()).unwrap();
    assert!(r.cpu_stat.is_some());
    assert!(r.cpu_pressure.is_some());
    assert!(r.memory_pressure.is_some());
    assert!(r.io_stat.is_some());
    assert!(r.io_pressure.is_some());
    // not captured at the root level in our fixture:
    assert!(r.memory_current.is_none());
    assert!(r.memory_events.is_none());
}
