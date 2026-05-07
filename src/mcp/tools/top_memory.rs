use crate::collector::stats::read_memory_current;
use crate::collector::tree::{read_cgroup_tree, CgroupKind, CgroupNode};
use crate::mcp::util::resolve_cgroup_dir;
use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::Path;

const DEFAULT_N: usize = 10;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct TopMemoryParams {
    /// Subtree to search under, relative to the cgroup root. Use the empty
    /// string `""` to search the whole tree. Examples: `""`,
    /// `"system.slice"`, `"user.slice"`. The path must not be absolute and
    /// must not contain `..` segments.
    #[serde(default)]
    pub path: String,

    /// Number of top consumers to return. Defaults to 10. Slices and the
    /// root cgroup are always excluded — their memory.current shows the
    /// summed memory of all descendants, which would dominate the ranking
    /// without representing actual leaf consumers.
    #[serde(default)]
    pub n: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TopMemoryEntry {
    /// Path relative to the cgroup root, e.g. `system.slice/nginx.service`.
    pub path: String,
    /// Cgroup kind (typically `service` or `scope`; `slice` and `root` are
    /// excluded by this tool).
    pub kind: CgroupKind,
    /// Current resident memory in bytes (`memory.current`).
    pub memory_current_bytes: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TopMemoryResponse {
    /// Echoes the queried subtree path.
    pub subtree: String,
    /// Top consumers, sorted descending by `memory_current_bytes`.
    pub results: Vec<TopMemoryEntry>,
}

pub fn run(cgroup_root: &Path, params: TopMemoryParams) -> Result<TopMemoryResponse> {
    let subtree_dir = resolve_cgroup_dir(cgroup_root, &params.path)?;
    let tree = read_cgroup_tree(&subtree_dir, None)?;
    let n = params.n.unwrap_or(DEFAULT_N).max(1);

    let mut entries = Vec::new();
    collect(&tree, &subtree_dir, &params.path, &mut entries)?;
    entries.sort_by_key(|e| std::cmp::Reverse(e.memory_current_bytes));
    entries.truncate(n);

    Ok(TopMemoryResponse {
        subtree: params.path,
        results: entries,
    })
}

fn collect(
    node: &CgroupNode,
    subtree_dir: &Path,
    subtree_rel: &str,
    out: &mut Vec<TopMemoryEntry>,
) -> Result<()> {
    if is_consumer_kind(&node.kind) {
        let dir = subtree_dir.join(&node.relative_path);
        if let Some(bytes) = read_memory_current_or_none(&dir)? {
            out.push(TopMemoryEntry {
                path: join_paths(subtree_rel, &node.relative_path),
                kind: node.kind.clone(),
                memory_current_bytes: bytes,
            });
        }
    }
    for child in &node.children {
        collect(child, subtree_dir, subtree_rel, out)?;
    }
    Ok(())
}

fn is_consumer_kind(k: &CgroupKind) -> bool {
    !matches!(k, CgroupKind::Root | CgroupKind::Slice)
}

/// Build a cgroup-root-relative path by joining the subtree we're searching
/// under with the per-node path returned by the tree walker.
fn join_paths(subtree_rel: &str, node_rel: &str) -> String {
    match (subtree_rel.is_empty(), node_rel.is_empty()) {
        (true, _) => node_rel.to_string(),
        (false, true) => subtree_rel.to_string(),
        (false, false) => format!("{subtree_rel}/{node_rel}"),
    }
}

fn read_memory_current_or_none(cgroup_dir: &Path) -> Result<Option<u64>> {
    match read_memory_current(&cgroup_dir.join("memory.current")) {
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
