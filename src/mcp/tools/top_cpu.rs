use crate::collector::rate::cpu_rate;
use crate::collector::stats::{read_cpu_stat, CpuStat};
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
pub struct TopCpuParams {
    /// Subtree to search under, relative to the cgroup root. Use the empty
    /// string `""` to search the whole tree. Examples: `""`,
    /// `"system.slice"`, `"user.slice"`. The path must not be absolute and
    /// must not contain `..` segments.
    #[serde(default)]
    pub path: String,

    /// Number of top consumers to return. Defaults to 10. Slices and the
    /// root cgroup are always excluded — their cpu.stat is summed across
    /// descendants and would dominate the ranking without representing
    /// actual leaf consumers.
    #[serde(default)]
    pub n: Option<usize>,

    /// Sample window in milliseconds. The tool blocks for this long while
    /// it reads cpu.stat twice (before and after) to compute a rate.
    /// Defaults to 500. Lower values respond faster but become noisy:
    /// kernel CPU accounting updates on context switches and timer ticks,
    /// so windows under ~50ms are dominated by quantization. Bursty
    /// workloads also alias badly under short windows. Higher values
    /// improve stability at the cost of agent latency.
    #[serde(default)]
    pub sample_window_ms: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TopCpuEntry {
    /// Path relative to the cgroup root, e.g. `system.slice/nginx.service`.
    pub path: String,
    /// Cgroup kind (typically `service` or `scope`; `slice` and `root` are
    /// excluded by this tool).
    pub kind: CgroupKind,
    /// CPU consumed during the sample window, expressed in cores. 1.0 = one
    /// full core saturated for the whole window. Can exceed 1.0 on
    /// multi-core systems.
    pub usage_cores: f64,
    /// Same data as `usage_cores`, raw. Microseconds of CPU time consumed
    /// during the window (delta of cpu.stat usage_usec).
    pub usage_delta_usec: u64,
    /// User-mode CPU as a fraction of cores during the window.
    pub user_cores: f64,
    /// Kernel-mode CPU as a fraction of cores during the window.
    pub system_cores: f64,
    /// Number of times the cgroup's CPU bandwidth limit fired during the
    /// window (delta of cpu.stat nr_throttled). Non-zero means the cgroup
    /// is hitting its limit; combine with `usage_cores` to distinguish
    /// "wants more CPU but capped" from "just has high demand."
    pub throttled_periods_delta: u64,
    /// Total time the cgroup spent throttled during the window
    /// (delta of cpu.stat throttled_usec).
    pub throttled_usec_delta: u64,
    /// True if any counter went backwards between snapshots — typically a
    /// cgroup recreation (service restart). All rate fields are 0 when
    /// this is set.
    pub reset_detected: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TopCpuResponse {
    /// Echoes the queried subtree path.
    pub subtree: String,
    /// Actual sample window used, in milliseconds.
    pub sample_window_ms: u64,
    /// Top consumers, sorted descending by `usage_delta_usec`.
    pub results: Vec<TopCpuEntry>,
}

pub async fn run(cgroup_root: &Path, params: TopCpuParams) -> Result<TopCpuResponse> {
    let subtree_dir = resolve_cgroup_dir(cgroup_root, &params.path)?;
    let tree = read_cgroup_tree(&subtree_dir, None)?;
    let n = params.n.unwrap_or(DEFAULT_N).max(1);
    let window_ms = params.sample_window_ms.unwrap_or(DEFAULT_WINDOW_MS);
    if window_ms == 0 {
        bail!("sample_window_ms must be > 0");
    }

    // Collect (path, kind, dir) for every cgroup we want to rank. Done up
    // front so the two read passes iterate the same set.
    let mut targets: Vec<TargetCgroup> = Vec::new();
    collect_targets(&tree, &subtree_dir, &params.path, &mut targets);

    let before = sample_all(&targets);
    sleep(Duration::from_millis(window_ms)).await;
    let after = sample_all(&targets);

    let dt = Duration::from_millis(window_ms);
    let mut entries: Vec<TopCpuEntry> = Vec::with_capacity(targets.len());
    for (i, target) in targets.iter().enumerate() {
        // Skip cgroups that failed either read (cgroup disappeared,
        // accounting not enabled, etc.).
        let Some(a) = &before[i] else { continue };
        let Some(b) = &after[i] else { continue };
        let rate = cpu_rate(a, b, dt)?;
        let (throttled_periods_delta, throttled_usec_delta) = if rate.reset_detected {
            (0, 0)
        } else {
            (
                b.nr_throttled.saturating_sub(a.nr_throttled),
                b.throttled_usec.saturating_sub(a.throttled_usec),
            )
        };
        entries.push(TopCpuEntry {
            path: target.relative_path.clone(),
            kind: target.kind.clone(),
            usage_cores: rate.usage_ratio,
            usage_delta_usec: rate.usage_delta_usec,
            user_cores: rate.user_ratio,
            system_cores: rate.system_ratio,
            throttled_periods_delta,
            throttled_usec_delta,
            reset_detected: rate.reset_detected,
        });
    }

    entries.sort_by_key(|e| std::cmp::Reverse(e.usage_delta_usec));
    entries.truncate(n);

    Ok(TopCpuResponse {
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

fn sample_all(targets: &[TargetCgroup]) -> Vec<Option<CpuStat>> {
    targets
        .iter()
        .map(|t| read_cpu_stat_or_none(&t.dir))
        .collect()
}

fn read_cpu_stat_or_none(cgroup_dir: &Path) -> Option<CpuStat> {
    match read_cpu_stat(&cgroup_dir.join("cpu.stat")) {
        Ok(v) => Some(v),
        Err(e) => {
            for cause in e.chain() {
                if let Some(io) = cause.downcast_ref::<std::io::Error>() {
                    if io.kind() == ErrorKind::NotFound {
                        return None;
                    }
                }
            }
            // Other errors (parse, permission) treated like absent — the
            // tool's job is to rank what it can read, not to fail the
            // whole call because one cgroup is unreadable.
            None
        }
    }
}

fn join_paths(subtree_rel: &str, node_rel: &str) -> String {
    match (subtree_rel.is_empty(), node_rel.is_empty()) {
        (true, _) => node_rel.to_string(),
        (false, true) => subtree_rel.to_string(),
        (false, false) => format!("{subtree_rel}/{node_rel}"),
    }
}
