use crate::collector::pressure::{read_pressure, Pressure};
use crate::collector::stats::{
    read_cpu_stat, read_io_stat, read_memory_current, read_memory_events, read_memory_stat,
    CpuStat, IoDeviceStat, MemoryEvents, MemoryStat,
};
use anyhow::Result;
use serde::Serialize;
use std::io::ErrorKind;
use std::path::Path;

/// Bundle of all stat files for a single cgroup. Each field is `Option<T>`
/// because controllers may not be enabled, accounting may be off, or the
/// file may simply not exist on the root vs. a leaf cgroup. Missing file =
/// `None`; parse errors propagate.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResourceStats {
    pub cpu_stat: Option<CpuStat>,
    pub cpu_pressure: Option<Pressure>,
    pub memory_current: Option<u64>,
    pub memory_stat: Option<MemoryStat>,
    pub memory_events: Option<MemoryEvents>,
    pub memory_pressure: Option<Pressure>,
    pub io_stat: Option<Vec<IoDeviceStat>>,
    pub io_pressure: Option<Pressure>,
}

pub fn read_resource_stats(cgroup_dir: &Path) -> Result<ResourceStats> {
    Ok(ResourceStats {
        cpu_stat: optional(read_cpu_stat(&cgroup_dir.join("cpu.stat")))?,
        cpu_pressure: optional(read_pressure(&cgroup_dir.join("cpu.pressure")))?,
        memory_current: optional(read_memory_current(&cgroup_dir.join("memory.current")))?,
        memory_stat: optional(read_memory_stat(&cgroup_dir.join("memory.stat")))?,
        memory_events: optional(read_memory_events(&cgroup_dir.join("memory.events")))?,
        memory_pressure: optional(read_pressure(&cgroup_dir.join("memory.pressure")))?,
        io_stat: optional(read_io_stat(&cgroup_dir.join("io.stat")))?,
        io_pressure: optional(read_pressure(&cgroup_dir.join("io.pressure")))?,
    })
}

/// Convert `Result<T>` to `Result<Option<T>>`, mapping NotFound IO errors
/// to None. Parse errors and other IO errors (permission, etc.) propagate.
fn optional<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            for cause in e.chain() {
                if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
                    if io_err.kind() == ErrorKind::NotFound {
                        return Ok(None);
                    }
                }
            }
            Err(e)
        }
    }
}
