use crate::collector::pressure::Pressure;
use crate::collector::resource_stats::read_resource_stats;
use crate::collector::stats::{CpuStat, IoDeviceStat, MemoryEvents, MemoryStat};
use crate::mcp::util::resolve_cgroup_dir;
use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetUnitStatsParams {
    /// Cgroup path relative to the configured cgroup root.
    /// Use the empty string `""` for the root cgroup. Examples: `""`,
    /// `"system.slice"`, `"system.slice/nginx.service"`. The path must not
    /// be absolute and must not contain `..` segments.
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GetUnitStatsResponse {
    /// Echoes the queried cgroup path.
    pub path: String,
    /// CPU accounting and pressure for this cgroup.
    pub cpu: CpuSection,
    /// Memory accounting, events, and pressure for this cgroup.
    pub memory: MemorySection,
    /// IO accounting (per-device) and pressure for this cgroup.
    pub io: IoSection,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CpuSection {
    /// Cumulative CPU usage from `cpu.stat`. Values are monotonic counters in
    /// microseconds; subtract two snapshots to get a rate. Null if cpu
    /// accounting is not available on this cgroup.
    pub stat: Option<CpuStat>,
    /// PSI from `cpu.pressure`. Null if the controller is not enabled.
    pub pressure: Option<Pressure>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct MemorySection {
    /// Current resident memory in bytes from `memory.current`. For slices
    /// and the root, this is summed across descendants. Null if memory
    /// accounting is not available (e.g. on the root cgroup of some
    /// kernels).
    pub current_bytes: Option<u64>,
    /// Full `memory.stat` as a map of kernel field name to value (bytes for
    /// most fields, counts for fault-style fields). The set of keys depends
    /// on kernel version; common ones include `anon`, `file`, `kernel`,
    /// `slab`, `sock`, `shmem`, `pgfault`, `pgmajfault`. Null if the file
    /// is absent.
    pub stat: Option<MemoryStat>,
    /// OOM and limit-event counters from `memory.events`. Cumulative since
    /// the cgroup was created. Null if the file is absent.
    pub events: Option<MemoryEvents>,
    /// PSI from `memory.pressure`. Null if the controller is not enabled.
    pub pressure: Option<Pressure>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct IoSection {
    /// Per-device IO counters from `io.stat`. Null if the IO controller is
    /// not enabled on this cgroup.
    pub stat: Option<Vec<IoDeviceStat>>,
    /// PSI from `io.pressure`. Null if the controller is not enabled.
    pub pressure: Option<Pressure>,
}

pub fn run(cgroup_root: &Path, params: GetUnitStatsParams) -> Result<GetUnitStatsResponse> {
    let cgroup_dir = resolve_cgroup_dir(cgroup_root, &params.path)?;
    let raw = read_resource_stats(&cgroup_dir)?;
    Ok(GetUnitStatsResponse {
        path: params.path,
        cpu: CpuSection {
            stat: raw.cpu_stat,
            pressure: raw.cpu_pressure,
        },
        memory: MemorySection {
            current_bytes: raw.memory_current,
            stat: raw.memory_stat,
            events: raw.memory_events,
            pressure: raw.memory_pressure,
        },
        io: IoSection {
            stat: raw.io_stat,
            pressure: raw.io_pressure,
        },
    })
}
