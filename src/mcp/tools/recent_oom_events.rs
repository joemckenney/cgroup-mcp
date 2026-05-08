use crate::collector::stats::{read_memory_events, MemoryEvents};
use crate::collector::tree::{read_cgroup_tree, CgroupKind, CgroupNode};
use crate::mcp::util::resolve_cgroup_dir;
use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RecentOomEventsParams {
    /// Subtree to search under, relative to the cgroup root. Use the empty
    /// string `""` to search the whole tree. Examples: `""`,
    /// `"system.slice"`, `"user.slice"`. The path must not be absolute and
    /// must not contain `..` segments.
    #[serde(default)]
    pub path: String,

    /// If true, include cgroups whose counters are all zero. Defaults to
    /// false: returning only cgroups that have actually hit a memory event
    /// keeps the response focused. Set to true if you want to confirm
    /// "nothing has OOMed" or to walk the full tree for some other reason.
    #[serde(default)]
    pub include_zero: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct OomEventEntry {
    /// Path relative to the cgroup root, e.g. `system.slice/nginx.service`.
    pub path: String,
    /// Cgroup kind.
    pub kind: CgroupKind,
    /// Counters from `memory.events.local` for this cgroup. Cumulative
    /// since the cgroup was created — no timestamp, no rolling window.
    pub events: MemoryEvents,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RecentOomEventsResponse {
    /// Echoes the queried subtree path.
    pub subtree: String,
    /// Matching cgroups, sorted by `oom_kill` desc, then `oom` desc, then
    /// `oom_group_kill`, `high`, `low` as tiebreakers.
    pub results: Vec<OomEventEntry>,
}

pub fn run(cgroup_root: &Path, params: RecentOomEventsParams) -> Result<RecentOomEventsResponse> {
    let subtree_dir = resolve_cgroup_dir(cgroup_root, &params.path)?;
    let tree = read_cgroup_tree(&subtree_dir, None)?;
    let include_zero = params.include_zero.unwrap_or(false);

    let mut entries = Vec::new();
    collect(
        &tree,
        &subtree_dir,
        &params.path,
        include_zero,
        &mut entries,
    )?;

    entries.sort_by_key(|e| {
        std::cmp::Reverse((
            e.events.oom_kill,
            e.events.oom,
            e.events.oom_group_kill,
            e.events.high,
            e.events.low,
        ))
    });

    Ok(RecentOomEventsResponse {
        subtree: params.path,
        results: entries,
    })
}

fn collect(
    node: &CgroupNode,
    subtree_dir: &Path,
    subtree_rel: &str,
    include_zero: bool,
    out: &mut Vec<OomEventEntry>,
) -> Result<()> {
    let dir = subtree_dir.join(&node.relative_path);
    if let Some(events) = read_memory_events_local_or_none(&dir)? {
        if include_zero || !is_all_zero(&events) {
            out.push(OomEventEntry {
                path: join_paths(subtree_rel, &node.relative_path),
                kind: node.kind.clone(),
                events,
            });
        }
    }
    for child in &node.children {
        collect(child, subtree_dir, subtree_rel, include_zero, out)?;
    }
    Ok(())
}

fn is_all_zero(e: &MemoryEvents) -> bool {
    e.low == 0
        && e.high == 0
        && e.max == 0
        && e.oom == 0
        && e.oom_kill == 0
        && e.oom_group_kill == 0
}

fn join_paths(subtree_rel: &str, node_rel: &str) -> String {
    match (subtree_rel.is_empty(), node_rel.is_empty()) {
        (true, _) => node_rel.to_string(),
        (false, true) => subtree_rel.to_string(),
        (false, false) => format!("{subtree_rel}/{node_rel}"),
    }
}

fn read_memory_events_local_or_none(cgroup_dir: &Path) -> Result<Option<MemoryEvents>> {
    match read_memory_events(&cgroup_dir.join("memory.events.local")) {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            for cause in e.chain() {
                if let Some(io) = cause.downcast_ref::<std::io::Error>() {
                    if io.kind() == ErrorKind::NotFound {
                        return Ok(None);
                    }
                }
            }
            Err(e)
        }
    }
}
