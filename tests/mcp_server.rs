use cgroup_mcp::mcp::server::CgroupServer;
use cgroup_mcp::mcp::tools::get_pressure::GetPressureParams;
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
