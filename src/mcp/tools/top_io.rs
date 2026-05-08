use crate::collector::rate::{io_rates, IoRate};
use crate::collector::stats::{read_io_stat, IoDeviceStat};
use crate::collector::tree::{read_cgroup_tree, CgroupKind, CgroupNode};
use crate::mcp::util::resolve_cgroup_dir;
use anyhow::{bail, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::sleep;

const DEFAULT_N: usize = 10;
const DEFAULT_WINDOW_MS: u64 = 500;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct TopIoParams {
    /// Subtree to search under, relative to the cgroup root. Use the empty
    /// string `""` to search the whole tree. Examples: `""`,
    /// `"system.slice"`, `"user.slice"`. The path must not be absolute and
    /// must not contain `..` segments.
    #[serde(default)]
    pub path: String,

    /// Number of top consumers to return. Defaults to 10. Slices and the
    /// root cgroup are always excluded since their io.stat is summed
    /// across descendants.
    #[serde(default)]
    pub n: Option<usize>,

    /// Sample window in milliseconds. The tool blocks for this long while
    /// it reads io.stat twice (before and after) to compute rates.
    /// Defaults to 500. Same tradeoff as top_cpu: lower values respond
    /// faster but become noisy on bursty IO; higher values smooth out at
    /// the cost of agent latency.
    #[serde(default)]
    pub sample_window_ms: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TopIoEntry {
    /// Path relative to the cgroup root, e.g. `system.slice/nginx.service`.
    pub path: String,
    /// Cgroup kind (typically `service` or `scope`; `slice` and `root` are
    /// excluded by this tool).
    pub kind: CgroupKind,
    /// Read+write bytes per second across all block devices, derived from
    /// `io.stat` deltas over the sample window. This is the headline rank
    /// field.
    pub total_bytes_per_sec: f64,
    /// Read bytes per second, summed across devices.
    pub rbytes_per_sec: f64,
    /// Write bytes per second, summed across devices.
    pub wbytes_per_sec: f64,
    /// Read+write IOPS across all devices.
    pub total_ios_per_sec: f64,
    /// Read IOPS, summed across devices.
    pub rios_per_sec: f64,
    /// Write IOPS, summed across devices.
    pub wios_per_sec: f64,
    /// Per-device breakdown. Useful when one cgroup is hammering a single
    /// device but quiet elsewhere; agents asking "what's hammering disk
    /// overall" can ignore this and read the totals.
    pub per_device: Vec<TopIoDeviceRate>,
    /// True if any counter went backwards on any device, typically due to
    /// a cgroup recreation. The aggregate rate fields are 0.0 when this
    /// is set; per-device entries flag the same condition individually.
    pub reset_detected: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TopIoDeviceRate {
    /// Device major number from `io.stat` line.
    pub major: u32,
    /// Device minor number from `io.stat` line.
    pub minor: u32,
    pub rbytes_per_sec: f64,
    pub wbytes_per_sec: f64,
    pub rios_per_sec: f64,
    pub wios_per_sec: f64,
    /// Discard bytes (TRIM/discard) per second.
    pub dbytes_per_sec: f64,
    pub dios_per_sec: f64,
    /// True if any counter on this device went backwards.
    pub reset_detected: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TopIoResponse {
    /// Echoes the queried subtree path.
    pub subtree: String,
    /// Actual sample window used, in milliseconds.
    pub sample_window_ms: u64,
    /// Top consumers, sorted descending by `total_bytes_per_sec`.
    pub results: Vec<TopIoEntry>,
}

pub async fn run(cgroup_root: &Path, params: TopIoParams) -> Result<TopIoResponse> {
    let subtree_dir = resolve_cgroup_dir(cgroup_root, &params.path)?;
    let tree = read_cgroup_tree(&subtree_dir, None)?;
    let n = params.n.unwrap_or(DEFAULT_N).max(1);
    let window_ms = params.sample_window_ms.unwrap_or(DEFAULT_WINDOW_MS);
    if window_ms == 0 {
        bail!("sample_window_ms must be > 0");
    }

    let mut targets: Vec<TargetCgroup> = Vec::new();
    collect_targets(&tree, &subtree_dir, &params.path, &mut targets);

    let before = sample_all(&targets);
    sleep(Duration::from_millis(window_ms)).await;
    let after = sample_all(&targets);

    let dt = Duration::from_millis(window_ms);
    let mut entries: Vec<TopIoEntry> = Vec::with_capacity(targets.len());
    for (i, target) in targets.iter().enumerate() {
        // io.stat may be absent (no IO controller) on some cgroups; skip
        // those silently rather than failing the whole call.
        let Some(a) = &before[i] else { continue };
        let Some(b) = &after[i] else { continue };
        let rates = io_rates(a, b, dt)?;
        entries.push(build_entry(target, rates));
    }

    entries.sort_by(|a, b| {
        b.total_bytes_per_sec
            .partial_cmp(&a.total_bytes_per_sec)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    entries.truncate(n);

    Ok(TopIoResponse {
        subtree: params.path,
        sample_window_ms: window_ms,
        results: entries,
    })
}

struct TargetCgroup {
    relative_path: String,
    kind: CgroupKind,
    dir: PathBuf,
}

fn collect_targets(
    node: &CgroupNode,
    subtree_dir: &Path,
    subtree_rel: &str,
    out: &mut Vec<TargetCgroup>,
) {
    if is_consumer_kind(&node.kind) {
        out.push(TargetCgroup {
            relative_path: join_paths(subtree_rel, &node.relative_path),
            kind: node.kind.clone(),
            dir: subtree_dir.join(&node.relative_path),
        });
    }
    for child in &node.children {
        collect_targets(child, subtree_dir, subtree_rel, out);
    }
}

fn is_consumer_kind(k: &CgroupKind) -> bool {
    !matches!(k, CgroupKind::Root | CgroupKind::Slice)
}

fn sample_all(targets: &[TargetCgroup]) -> Vec<Option<Vec<IoDeviceStat>>> {
    targets
        .iter()
        .map(|t| read_io_stat_or_none(&t.dir))
        .collect()
}

fn read_io_stat_or_none(cgroup_dir: &Path) -> Option<Vec<IoDeviceStat>> {
    match read_io_stat(&cgroup_dir.join("io.stat")) {
        Ok(v) => Some(v),
        Err(e) => {
            for cause in e.chain() {
                if let Some(io) = cause.downcast_ref::<std::io::Error>() {
                    if io.kind() == ErrorKind::NotFound {
                        return None;
                    }
                }
            }
            None
        }
    }
}

fn build_entry(target: &TargetCgroup, rates: Vec<IoRate>) -> TopIoEntry {
    // Aggregate across devices. A device whose counters reset contributes
    // 0 to the totals (io_rates already zeroed its rate fields), and we
    // surface reset_detected at the entry level if any device reset.
    let mut rbytes = 0.0;
    let mut wbytes = 0.0;
    let mut rios = 0.0;
    let mut wios = 0.0;
    let mut any_reset = false;
    let per_device: Vec<TopIoDeviceRate> = rates
        .into_iter()
        .map(|r| {
            rbytes += r.rbytes_per_sec;
            wbytes += r.wbytes_per_sec;
            rios += r.rios_per_sec;
            wios += r.wios_per_sec;
            any_reset |= r.reset_detected;
            TopIoDeviceRate {
                major: r.major,
                minor: r.minor,
                rbytes_per_sec: r.rbytes_per_sec,
                wbytes_per_sec: r.wbytes_per_sec,
                rios_per_sec: r.rios_per_sec,
                wios_per_sec: r.wios_per_sec,
                dbytes_per_sec: r.dbytes_per_sec,
                dios_per_sec: r.dios_per_sec,
                reset_detected: r.reset_detected,
            }
        })
        .collect();
    TopIoEntry {
        path: target.relative_path.clone(),
        kind: target.kind.clone(),
        total_bytes_per_sec: rbytes + wbytes,
        rbytes_per_sec: rbytes,
        wbytes_per_sec: wbytes,
        total_ios_per_sec: rios + wios,
        rios_per_sec: rios,
        wios_per_sec: wios,
        per_device,
        reset_detected: any_reset,
    }
}

fn join_paths(subtree_rel: &str, node_rel: &str) -> String {
    match (subtree_rel.is_empty(), node_rel.is_empty()) {
        (true, _) => node_rel.to_string(),
        (false, true) => subtree_rel.to_string(),
        (false, false) => format!("{subtree_rel}/{node_rel}"),
    }
}
