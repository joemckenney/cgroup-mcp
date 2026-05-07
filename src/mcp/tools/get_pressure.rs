use crate::collector::pressure::{read_pressure, Pressure};
use crate::mcp::util::resolve_cgroup_dir;
use anyhow::{Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetPressureParams {
    /// Cgroup path relative to the configured cgroup root.
    /// Use the empty string `""` for system-wide pressure (the root cgroup).
    /// Examples: `""`, `"system.slice"`, `"system.slice/nginx.service"`.
    /// The path must not be absolute and must not contain `..` segments.
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GetPressureResponse {
    /// Echoes the queried cgroup path so the agent can correlate the response.
    pub path: String,
    /// Memory pressure: time tasks were stalled waiting for memory.
    pub memory: Option<Pressure>,
    /// CPU pressure: time tasks were stalled waiting for CPU.
    pub cpu: Option<Pressure>,
    /// IO pressure: time tasks were stalled waiting for IO.
    pub io: Option<Pressure>,
}

pub fn run(cgroup_root: &Path, params: GetPressureParams) -> Result<GetPressureResponse> {
    let cgroup_dir = resolve_cgroup_dir(cgroup_root, &params.path)?;
    Ok(GetPressureResponse {
        path: params.path,
        memory: optional_psi(&cgroup_dir.join("memory.pressure"))?,
        cpu: optional_psi(&cgroup_dir.join("cpu.pressure"))?,
        io: optional_psi(&cgroup_dir.join("io.pressure"))?,
    })
}

fn optional_psi(path: &Path) -> Result<Option<Pressure>> {
    match read_pressure(path) {
        Ok(p) => Ok(Some(p)),
        Err(e) => {
            for cause in e.chain() {
                if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
                    if io_err.kind() == ErrorKind::NotFound {
                        return Ok(None);
                    }
                }
            }
            Err(e).with_context(|| format!("reading {}", path.display()))
        }
    }
}
