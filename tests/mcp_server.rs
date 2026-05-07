use cgroup_mcp::collector::tree::CgroupKind;
use cgroup_mcp::mcp::server::CgroupServer;
use cgroup_mcp::mcp::tools::get_pressure::GetPressureParams;
use cgroup_mcp::mcp::tools::top_memory::TopMemoryParams;
use rmcp::handler::server::wrapper::Parameters;
use std::path::{Path, PathBuf};

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
        .get_pressure(Parameters(GetPressureParams { path: String::new() }))
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
    assert!(
        format!("{err}").contains("absolute"),
        "error was: {err}"
    );
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
    assert!(
        format!("{err}").contains("not found"),
        "error was: {err}"
    );
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
        !resp.results.iter().any(|e| e.path.contains("system-getty.slice")),
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
    assert_eq!(resp.0.results[0].path, "system.slice/systemd-journald.service");
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
