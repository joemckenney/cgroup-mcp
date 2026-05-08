use cgroup_mcp::collector::tree::CgroupKind;
use cgroup_mcp::mcp::server::CgroupServer;
use cgroup_mcp::mcp::tools::get_pressure::GetPressureParams;
use cgroup_mcp::mcp::tools::get_unit_stats::GetUnitStatsParams;
use cgroup_mcp::mcp::tools::recent_oom_events::RecentOomEventsParams;
use cgroup_mcp::mcp::tools::top_cpu::TopCpuParams;
use cgroup_mcp::mcp::tools::top_memory::TopMemoryParams;
use rmcp::handler::server::wrapper::Parameters;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn real_arch_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/real_arch")
}

#[tokio::test]
async fn tool_list_snapshot() {
    // Locks the public tool surface (names, descriptions, schemas) against
    // unintentional drift. Tool descriptions are how the LLM picks tools,
    // so any change should be a deliberate, reviewable diff.
    let server = CgroupServer::new(real_arch_root());
    let mut tools = server.list_tools();
    tools.sort_by(|a, b| a.name.cmp(&b.name));

    // Reduce to the fields that matter for the contract; the full Tool
    // struct includes things we don't want to snapshot (annotations etc.).
    let summary: Vec<_> = tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
                "output_schema": t.output_schema,
            })
        })
        .collect();
    insta::assert_yaml_snapshot!(summary);
}

#[tokio::test]
async fn get_pressure_for_named_unit_returns_three_psi_stanzas() {
    let server = CgroupServer::new(real_arch_root());
    let resp = server
        .get_pressure(Parameters(GetPressureParams {
            path: "system.slice/dbus-broker.service".into(),
        }))
        .await
        .expect("get_pressure should succeed");
    let resp = resp.0;

    assert_eq!(resp.path, "system.slice/dbus-broker.service");
    let mem = resp.memory.expect("memory.pressure present in fixture");
    assert!(mem.full.is_some(), "memory PSI should have a 'full' line");
    let cpu = resp.cpu.expect("cpu.pressure present in fixture");
    // Modern kernels include a `full` line on cgroup-level cpu.pressure;
    // it's only `/proc/pressure/cpu` (system-wide, non-cgroup) that
    // historically lacked it — and even that may include it on recent kernels.
    assert!(cpu.some.total_usec > 0 || cpu.some.avg10 == 0.0);
    assert!(resp.io.is_some());
}

#[tokio::test]
async fn get_pressure_with_empty_path_targets_the_root_cgroup() {
    let server = CgroupServer::new(real_arch_root());
    let resp = server
        .get_pressure(Parameters(GetPressureParams {
            path: String::new(),
        }))
        .await
        .expect("system-wide pressure should succeed");
    assert_eq!(resp.0.path, "");
    // The captured root has all three pressure files.
    assert!(resp.0.memory.is_some());
    assert!(resp.0.cpu.is_some());
    assert!(resp.0.io.is_some());
}

#[tokio::test]
async fn get_pressure_rejects_absolute_paths() {
    let server = CgroupServer::new(real_arch_root());
    let err = server
        .get_pressure(Parameters(GetPressureParams {
            path: "/etc/passwd".into(),
        }))
        .await
        .err()
        .expect("expected an error");
    assert!(format!("{err}").contains("absolute"), "error was: {err}");
}

#[tokio::test]
async fn get_pressure_rejects_dotdot_traversal() {
    let server = CgroupServer::new(real_arch_root());
    let err = server
        .get_pressure(Parameters(GetPressureParams {
            path: "system.slice/../../etc".into(),
        }))
        .await
        .err()
        .expect("expected an error");
    assert!(format!("{err}").contains(".."), "error was: {err}");
}

#[tokio::test]
async fn get_pressure_returns_error_for_missing_cgroup() {
    let server = CgroupServer::new(real_arch_root());
    let err = server
        .get_pressure(Parameters(GetPressureParams {
            path: "nonexistent.slice/never.service".into(),
        }))
        .await
        .err()
        .expect("expected an error");
    assert!(format!("{err}").contains("not found"), "error was: {err}");
}

// ---- top_memory ----

#[tokio::test]
async fn top_memory_returns_services_descending_excluding_slices() {
    // The captured fixture has 3 services and 1 nested slice under
    // system.slice; the slice has its own memory.current (4.7MiB) but
    // should not appear because slice memory is summed-descendant memory.
    let server = CgroupServer::new(real_arch_root());
    let resp = server
        .top_memory(Parameters(TopMemoryParams {
            path: String::new(),
            n: None,
        }))
        .await
        .expect("top_memory should succeed");
    let resp = resp.0;

    // expected order from the fixture (bytes):
    //   systemd-journald.service  46_305_280
    //   NetworkManager.service    24_616_960
    //   dbus-broker.service        6_144_000
    //   system-getty.slice         4_923_392  (excluded — is a slice)
    let names: Vec<_> = resp.results.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "system.slice/systemd-journald.service",
            "system.slice/NetworkManager.service",
            "system.slice/dbus-broker.service",
        ]
    );

    assert!(
        resp.results.iter().all(|e| e.kind == CgroupKind::Service),
        "every result should be a service in this fixture"
    );

    // Sanity-check the byte values came through unmodified.
    assert_eq!(resp.results[0].memory_current_bytes, 46_305_280);
    assert_eq!(resp.results[2].memory_current_bytes, 6_144_000);

    // Slices must not be present in results.
    assert!(
        !resp
            .results
            .iter()
            .any(|e| e.path.contains("system-getty.slice")),
        "slice should be excluded"
    );
}

#[tokio::test]
async fn top_memory_n_caps_results() {
    let server = CgroupServer::new(real_arch_root());
    let resp = server
        .top_memory(Parameters(TopMemoryParams {
            path: String::new(),
            n: Some(1),
        }))
        .await
        .expect("top_memory n=1");
    assert_eq!(resp.0.results.len(), 1);
    assert_eq!(
        resp.0.results[0].path,
        "system.slice/systemd-journald.service"
    );
}

#[tokio::test]
async fn top_memory_with_subtree_returns_paths_relative_to_cgroup_root() {
    // Searching only under `system.slice` should still produce paths that
    // include the subtree prefix — agents can pass these straight back to
    // other tools without re-anchoring.
    let server = CgroupServer::new(real_arch_root());
    let resp = server
        .top_memory(Parameters(TopMemoryParams {
            path: "system.slice".into(),
            n: None,
        }))
        .await
        .expect("subtree top_memory");
    assert!(!resp.0.results.is_empty());
    for entry in &resp.0.results {
        assert!(
            entry.path.starts_with("system.slice/"),
            "expected cgroup-root-relative path, got {}",
            entry.path
        );
    }
    assert_eq!(resp.0.subtree, "system.slice");
}

#[tokio::test]
async fn top_memory_n_larger_than_available_is_fine() {
    let server = CgroupServer::new(real_arch_root());
    let resp = server
        .top_memory(Parameters(TopMemoryParams {
            path: String::new(),
            n: Some(1000),
        }))
        .await
        .expect("large n");
    // We only captured 3 services with memory.current; should get all of them.
    assert_eq!(resp.0.results.len(), 3);
}

#[tokio::test]
async fn top_memory_validates_path_like_get_pressure() {
    let server = CgroupServer::new(real_arch_root());

    let err = server
        .top_memory(Parameters(TopMemoryParams {
            path: "/etc".into(),
            n: None,
        }))
        .await
        .err()
        .expect("absolute path should fail");
    assert!(format!("{err}").contains("absolute"));

    let err = server
        .top_memory(Parameters(TopMemoryParams {
            path: "system.slice/..".into(),
            n: None,
        }))
        .await
        .err()
        .expect("dotdot should fail");
    assert!(format!("{err}").contains(".."));
}

// ---- get_unit_stats ----

#[tokio::test]
async fn get_unit_stats_returns_full_bundle_for_a_service() {
    // dbus-broker.service has all eight stat files captured in the fixture.
    let server = CgroupServer::new(real_arch_root());
    let resp = server
        .get_unit_stats(Parameters(GetUnitStatsParams {
            path: "system.slice/dbus-broker.service".into(),
        }))
        .await
        .expect("get_unit_stats should succeed");
    let resp = resp.0;

    assert_eq!(resp.path, "system.slice/dbus-broker.service");

    // CPU section
    let cpu_stat = resp.cpu.stat.expect("cpu.stat present");
    assert!(cpu_stat.usage_usec > 0);
    assert!(resp.cpu.pressure.is_some());

    // Memory section: all four fields populated for this fixture.
    let mem = &resp.memory;
    assert_eq!(mem.current_bytes, Some(6_144_000));
    let stat = mem.stat.as_ref().expect("memory.stat present");
    // Raw map should round-trip kernel field names verbatim.
    assert_eq!(stat.get("anon"), 4_902_912);
    assert_eq!(stat.get("file"), 811_008);
    assert!(
        stat.raw.contains_key("pgfault"),
        "memory.stat should retain less-common keys"
    );
    let events = mem.events.as_ref().expect("memory.events present");
    assert_eq!(events.oom, 0);
    assert_eq!(events.oom_kill, 0);
    assert!(mem.pressure.is_some());

    // IO section
    assert!(resp.io.pressure.is_some());
}

#[tokio::test]
async fn get_unit_stats_with_empty_path_targets_root_cgroup() {
    // The captured root has cpu/memory/io stat and pressure files, but no
    // memory.current and no memory.events — exercising the per-field null
    // behavior.
    let server = CgroupServer::new(real_arch_root());
    let resp = server
        .get_unit_stats(Parameters(GetUnitStatsParams {
            path: String::new(),
        }))
        .await
        .expect("get_unit_stats on root should succeed");
    let resp = resp.0;

    assert_eq!(resp.path, "");
    assert!(resp.cpu.stat.is_some());
    assert!(resp.cpu.pressure.is_some());

    assert!(
        resp.memory.current_bytes.is_none(),
        "root capture has no memory.current"
    );
    assert!(
        resp.memory.events.is_none(),
        "root capture has no memory.events"
    );
    assert!(resp.memory.stat.is_some());
    assert!(resp.memory.pressure.is_some());

    assert!(resp.io.stat.is_some());
    assert!(resp.io.pressure.is_some());
}

#[tokio::test]
async fn get_unit_stats_returns_error_for_missing_cgroup() {
    let server = CgroupServer::new(real_arch_root());
    let err = server
        .get_unit_stats(Parameters(GetUnitStatsParams {
            path: "nonexistent.slice/never.service".into(),
        }))
        .await
        .err()
        .expect("expected an error");
    assert!(format!("{err}").contains("not found"), "error was: {err}");
}

#[tokio::test]
async fn get_unit_stats_validates_path_like_other_tools() {
    let server = CgroupServer::new(real_arch_root());

    let err = server
        .get_unit_stats(Parameters(GetUnitStatsParams {
            path: "/etc/passwd".into(),
        }))
        .await
        .err()
        .expect("absolute path should fail");
    assert!(format!("{err}").contains("absolute"), "error was: {err}");

    let err = server
        .get_unit_stats(Parameters(GetUnitStatsParams {
            path: "system.slice/../../etc".into(),
        }))
        .await
        .err()
        .expect("dotdot should fail");
    assert!(format!("{err}").contains(".."), "error was: {err}");
}

// ---- recent_oom_events ----

/// Build a minimal synthetic tree where each cgroup has a `cgroup.controllers`
/// marker and an optional `memory.events.local` body. The body string is
/// written verbatim. None means "no memory.events.local file" (skipped node).
fn synthetic_oom_tree(entries: &[(&str, Option<&str>)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("cgroup.controllers"), "").unwrap();
    for (rel, body) in entries {
        let mut current = dir.path().to_path_buf();
        for component in Path::new(rel).components() {
            current.push(component);
            fs::create_dir_all(&current).unwrap();
            let marker = current.join("cgroup.controllers");
            if !marker.exists() {
                fs::write(marker, "").unwrap();
            }
        }
        if let Some(body) = body {
            fs::write(current.join("memory.events.local"), body).unwrap();
        }
    }
    dir
}

#[tokio::test]
async fn recent_oom_events_filters_zero_and_sorts_by_oom_kill_desc() {
    // killer.service has the highest oom_kill; spammer.service has high
    // counts on lower-priority fields; quiet.service is all zero (filtered).
    let dir = synthetic_oom_tree(&[
        (
            "system.slice/killer.service",
            Some("low 0\nhigh 0\nmax 0\noom 7\noom_kill 5\noom_group_kill 0\n"),
        ),
        (
            "system.slice/spammer.service",
            Some("low 9\nhigh 9\nmax 0\noom 1\noom_kill 1\noom_group_kill 0\n"),
        ),
        (
            "system.slice/quiet.service",
            Some("low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\noom_group_kill 0\n"),
        ),
    ]);
    let server = CgroupServer::new(dir.path().to_path_buf());
    let resp = server
        .recent_oom_events(Parameters(RecentOomEventsParams {
            path: String::new(),
            include_zero: None,
        }))
        .await
        .expect("recent_oom_events should succeed");
    let resp = resp.0;

    let names: Vec<_> = resp.results.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "system.slice/killer.service",
            "system.slice/spammer.service",
        ],
        "quiet.service should be filtered out; killer outranks spammer on oom_kill"
    );
    assert_eq!(resp.results[0].events.oom_kill, 5);
    assert_eq!(resp.results[1].events.oom_kill, 1);
}

#[tokio::test]
async fn recent_oom_events_include_zero_returns_all_cgroups_with_the_file() {
    let dir = synthetic_oom_tree(&[
        (
            "system.slice/quiet.service",
            Some("low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\noom_group_kill 0\n"),
        ),
        (
            "system.slice/no-events-file.service",
            None, // present in the tree but missing memory.events.local
        ),
    ]);
    let server = CgroupServer::new(dir.path().to_path_buf());
    let resp = server
        .recent_oom_events(Parameters(RecentOomEventsParams {
            path: String::new(),
            include_zero: Some(true),
        }))
        .await
        .expect("include_zero should succeed");
    let names: Vec<_> = resp.0.results.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        names,
        vec!["system.slice/quiet.service"],
        "missing memory.events.local => skipped silently; zero counters => kept under include_zero"
    );
}

#[tokio::test]
async fn recent_oom_events_on_healthy_real_arch_is_empty_by_default() {
    // The captured fixture is a healthy box: every memory.events.local is
    // all-zero. Default filter should produce an empty list — exactly the
    // signal an agent wants when answering "did anything OOM."
    let server = CgroupServer::new(real_arch_root());
    let resp = server
        .recent_oom_events(Parameters(RecentOomEventsParams {
            path: String::new(),
            include_zero: None,
        }))
        .await
        .expect("recent_oom_events should succeed on real_arch");
    assert!(
        resp.0.results.is_empty(),
        "expected empty on healthy fixture, got {:?}",
        resp.0.results
    );
}

#[tokio::test]
async fn recent_oom_events_with_include_zero_walks_real_arch() {
    // include_zero=true should surface every cgroup that has the file,
    // confirming the walk visits the expected cgroups.
    let server = CgroupServer::new(real_arch_root());
    let resp = server
        .recent_oom_events(Parameters(RecentOomEventsParams {
            path: String::new(),
            include_zero: Some(true),
        }))
        .await
        .expect("walk should succeed");
    let paths: std::collections::HashSet<_> =
        resp.0.results.iter().map(|e| e.path.clone()).collect();

    for expected in [
        "system.slice",
        "system.slice/dbus-broker.service",
        "system.slice/NetworkManager.service",
        "system.slice/systemd-journald.service",
        "system.slice/system-getty.slice",
    ] {
        assert!(
            paths.contains(expected),
            "expected {expected} in results, got {paths:?}"
        );
    }

    // Every entry on the healthy fixture should be all-zero.
    for entry in &resp.0.results {
        assert_eq!(entry.events.oom_kill, 0, "{}", entry.path);
        assert_eq!(entry.events.oom, 0, "{}", entry.path);
    }
}

#[tokio::test]
async fn recent_oom_events_validates_path_like_other_tools() {
    let server = CgroupServer::new(real_arch_root());

    let err = server
        .recent_oom_events(Parameters(RecentOomEventsParams {
            path: "/etc".into(),
            include_zero: None,
        }))
        .await
        .err()
        .expect("absolute path should fail");
    assert!(format!("{err}").contains("absolute"), "error was: {err}");

    let err = server
        .recent_oom_events(Parameters(RecentOomEventsParams {
            path: "system.slice/..".into(),
            include_zero: None,
        }))
        .await
        .err()
        .expect("dotdot should fail");
    assert!(format!("{err}").contains(".."), "error was: {err}");
}

#[tokio::test]
async fn recent_oom_events_kind_is_populated() {
    // Sanity: the entry should reflect the cgroup's parsed kind, not Other.
    let dir = synthetic_oom_tree(&[(
        "system.slice/killer.service",
        Some("low 0\nhigh 0\nmax 0\noom 1\noom_kill 1\noom_group_kill 0\n"),
    )]);
    let server = CgroupServer::new(dir.path().to_path_buf());
    let resp = server
        .recent_oom_events(Parameters(RecentOomEventsParams {
            path: String::new(),
            include_zero: None,
        }))
        .await
        .expect("ok");
    assert_eq!(resp.0.results[0].kind, CgroupKind::Service);
}

// ---- top_cpu ----

/// Build a synthetic cgroup tree where each entry gets a `cgroup.controllers`
/// marker and an initial `cpu.stat` body. Intermediate path components are
/// also materialized as cgroup directories without cpu.stat (so they show up
/// as kind=Slice and are ranked-out). Returns the tempdir.
fn synthetic_cpu_tree(entries: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("cgroup.controllers"), "").unwrap();
    for (rel, cpu_stat_body) in entries {
        let mut current = dir.path().to_path_buf();
        for component in Path::new(rel).components() {
            current.push(component);
            fs::create_dir_all(&current).unwrap();
            let marker = current.join("cgroup.controllers");
            if !marker.exists() {
                fs::write(marker, "").unwrap();
            }
        }
        fs::write(current.join("cpu.stat"), cpu_stat_body).unwrap();
    }
    dir
}

fn cpu_stat_body(
    usage: u64,
    user: u64,
    system: u64,
    nr_throttled: u64,
    throttled_usec: u64,
) -> String {
    format!(
        "usage_usec {usage}\nuser_usec {user}\nsystem_usec {system}\n\
         nice_usec 0\nnr_periods 0\nnr_throttled {nr_throttled}\n\
         throttled_usec {throttled_usec}\nnr_bursts 0\nburst_usec 0\n"
    )
}

#[tokio::test]
async fn top_cpu_ranks_by_usage_delta_and_excludes_slices() {
    // Initial values; the test rewrites cpu.stat partway through the
    // sampling window to simulate CPU consumption during the window.
    let dir = synthetic_cpu_tree(&[
        ("system.slice/heavy.service", &cpu_stat_body(0, 0, 0, 0, 0)),
        ("system.slice/light.service", &cpu_stat_body(0, 0, 0, 0, 0)),
        // system-getty.slice has its own cpu.stat (slices do); should be
        // excluded from results despite a delta.
        (
            "system.slice/system-getty.slice",
            &cpu_stat_body(0, 0, 0, 0, 0),
        ),
    ]);

    let dir_path = dir.path().to_path_buf();
    let mutator_path = dir_path.clone();

    // Update cpu.stat 100ms into a 300ms window. Tool reads at ~t=0 and ~t=300,
    // mutator runs at t=100, so the second read sees the AFTER values.
    let mutator = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        fs::write(
            mutator_path.join("system.slice/heavy.service/cpu.stat"),
            cpu_stat_body(150_000, 100_000, 50_000, 3, 6_000),
        )
        .unwrap();
        fs::write(
            mutator_path.join("system.slice/light.service/cpu.stat"),
            cpu_stat_body(15_000, 10_000, 5_000, 0, 0),
        )
        .unwrap();
        fs::write(
            mutator_path.join("system.slice/system-getty.slice/cpu.stat"),
            cpu_stat_body(999_999_999, 999_999_999, 0, 0, 0),
        )
        .unwrap();
    });

    let server = CgroupServer::new(dir_path);
    let resp = server
        .top_cpu(Parameters(TopCpuParams {
            path: String::new(),
            n: None,
            sample_window_ms: Some(300),
        }))
        .await
        .expect("top_cpu should succeed");
    mutator.await.unwrap();
    let resp = resp.0;

    assert_eq!(resp.sample_window_ms, 300);
    let names: Vec<_> = resp.results.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        names,
        vec!["system.slice/heavy.service", "system.slice/light.service"],
        "slice should be excluded; heavy outranks light on usage delta"
    );

    let heavy = &resp.results[0];
    assert_eq!(heavy.usage_delta_usec, 150_000);
    // 150_000us / 300ms = 0.5 cores
    assert!(
        (heavy.usage_cores - 0.5).abs() < 1e-9,
        "expected ~0.5 cores, got {}",
        heavy.usage_cores
    );
    assert_eq!(heavy.throttled_periods_delta, 3);
    assert_eq!(heavy.throttled_usec_delta, 6_000);
    assert!(!heavy.reset_detected);

    let light = &resp.results[1];
    assert_eq!(light.usage_delta_usec, 15_000);
    assert_eq!(light.throttled_periods_delta, 0);
}

#[tokio::test]
async fn top_cpu_n_caps_results() {
    let dir = synthetic_cpu_tree(&[
        ("system.slice/a.service", &cpu_stat_body(0, 0, 0, 0, 0)),
        ("system.slice/b.service", &cpu_stat_body(0, 0, 0, 0, 0)),
        ("system.slice/c.service", &cpu_stat_body(0, 0, 0, 0, 0)),
    ]);
    let dir_path = dir.path().to_path_buf();
    let mutator_path = dir_path.clone();
    let mutator = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        fs::write(
            mutator_path.join("system.slice/a.service/cpu.stat"),
            cpu_stat_body(30_000, 0, 0, 0, 0),
        )
        .unwrap();
        fs::write(
            mutator_path.join("system.slice/b.service/cpu.stat"),
            cpu_stat_body(20_000, 0, 0, 0, 0),
        )
        .unwrap();
        fs::write(
            mutator_path.join("system.slice/c.service/cpu.stat"),
            cpu_stat_body(10_000, 0, 0, 0, 0),
        )
        .unwrap();
    });

    let server = CgroupServer::new(dir_path);
    let resp = server
        .top_cpu(Parameters(TopCpuParams {
            path: String::new(),
            n: Some(2),
            sample_window_ms: Some(250),
        }))
        .await
        .expect("ok");
    mutator.await.unwrap();
    assert_eq!(resp.0.results.len(), 2);
    assert_eq!(resp.0.results[0].path, "system.slice/a.service");
    assert_eq!(resp.0.results[1].path, "system.slice/b.service");
}

#[tokio::test]
async fn top_cpu_zero_window_is_rejected() {
    let server = CgroupServer::new(real_arch_root());
    let err = server
        .top_cpu(Parameters(TopCpuParams {
            path: String::new(),
            n: None,
            sample_window_ms: Some(0),
        }))
        .await
        .err()
        .expect("zero window should fail");
    assert!(format!("{err}").contains("> 0"), "error was: {err}");
}

#[tokio::test]
async fn top_cpu_against_real_arch_returns_static_zero_rates() {
    // The captured fixture has identical cpu.stat values across both
    // reads, so every entry should report 0 usage and reset_detected=false.
    let server = CgroupServer::new(real_arch_root());
    let resp = server
        .top_cpu(Parameters(TopCpuParams {
            path: String::new(),
            n: None,
            sample_window_ms: Some(60),
        }))
        .await
        .expect("ok");
    let resp = resp.0;
    assert_eq!(resp.sample_window_ms, 60);
    // Captured fixture has 4 services in system.slice (none under
    // system-getty.slice in the fixture). All should appear, all with
    // zero usage.
    assert!(!resp.results.is_empty());
    for entry in &resp.results {
        assert!(
            !matches!(entry.kind, CgroupKind::Slice | CgroupKind::Root),
            "slice/root should be excluded: {} ({:?})",
            entry.path,
            entry.kind,
        );
        assert_eq!(entry.usage_delta_usec, 0);
        assert_eq!(entry.usage_cores, 0.0);
        assert!(!entry.reset_detected);
    }
}

#[tokio::test]
async fn top_cpu_handles_counter_reset() {
    // Cgroup recreation (service restart) makes counters go backwards.
    // The tool should flag reset_detected and zero out the rate fields
    // rather than emitting a giant negative-as-unsigned delta.
    let dir = synthetic_cpu_tree(&[(
        "system.slice/restarter.service",
        &cpu_stat_body(1_000_000, 700_000, 300_000, 5, 50_000),
    )]);
    let dir_path = dir.path().to_path_buf();
    let mutator_path = dir_path.clone();
    let mutator = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(60)).await;
        // Counter went backwards — restart.
        fs::write(
            mutator_path.join("system.slice/restarter.service/cpu.stat"),
            cpu_stat_body(50_000, 30_000, 20_000, 0, 0),
        )
        .unwrap();
    });

    let server = CgroupServer::new(dir_path);
    let resp = server
        .top_cpu(Parameters(TopCpuParams {
            path: String::new(),
            n: None,
            sample_window_ms: Some(200),
        }))
        .await
        .expect("ok");
    mutator.await.unwrap();
    let entry = &resp.0.results[0];
    assert!(entry.reset_detected, "should flag reset");
    assert_eq!(entry.usage_delta_usec, 0);
    assert_eq!(entry.usage_cores, 0.0);
    assert_eq!(entry.throttled_periods_delta, 0);
}

#[tokio::test]
async fn top_cpu_validates_path_like_other_tools() {
    let server = CgroupServer::new(real_arch_root());

    let err = server
        .top_cpu(Parameters(TopCpuParams {
            path: "/etc".into(),
            n: None,
            sample_window_ms: Some(50),
        }))
        .await
        .err()
        .expect("absolute path should fail");
    assert!(format!("{err}").contains("absolute"), "error was: {err}");

    let err = server
        .top_cpu(Parameters(TopCpuParams {
            path: "system.slice/..".into(),
            n: None,
            sample_window_ms: Some(50),
        }))
        .await
        .err()
        .expect("dotdot should fail");
    assert!(format!("{err}").contains(".."), "error was: {err}");
}
