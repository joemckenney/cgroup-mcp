use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CgroupNode {
    pub name: String,
    pub relative_path: String,
    pub kind: CgroupKind,
    pub children: Vec<CgroupNode>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CgroupKind {
    Root,
    Slice,
    Service,
    Scope,
    Mount,
    Socket,
    Other(String),
}

pub fn read_cgroup_tree(root: &Path, max_depth: Option<usize>) -> Result<CgroupNode> {
    if !is_cgroup_dir(root) {
        bail!(
            "not a cgroup directory (no cgroup.controllers): {}",
            root.display()
        );
    }
    walk(root, root, 0, max_depth)
}

fn walk(
    root: &Path,
    dir: &Path,
    depth: usize,
    max_depth: Option<usize>,
) -> Result<CgroupNode> {
    let name = if dir == root {
        String::new()
    } else {
        dir.file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .with_context(|| format!("non-utf8 cgroup name at {}", dir.display()))?
    };

    let relative_path = dir
        .strip_prefix(root)
        .with_context(|| format!("{} is not under root {}", dir.display(), root.display()))?
        .to_string_lossy()
        .to_string();

    let kind = if dir == root {
        CgroupKind::Root
    } else {
        kind_from_name(&name)
    };

    let at_limit = max_depth.is_some_and(|d| depth >= d);

    let (children, truncated) = if at_limit {
        (Vec::new(), has_cgroup_children(dir)?)
    } else {
        let mut child_dirs: Vec<PathBuf> = std::fs::read_dir(dir)
            .with_context(|| format!("reading directory {}", dir.display()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir() && is_cgroup_dir(p))
            .collect();
        child_dirs.sort();
        let mut nodes = Vec::with_capacity(child_dirs.len());
        for child in child_dirs {
            nodes.push(walk(root, &child, depth + 1, max_depth)?);
        }
        (nodes, false)
    };

    Ok(CgroupNode {
        name,
        relative_path,
        kind,
        children,
        truncated,
    })
}

fn is_cgroup_dir(p: &Path) -> bool {
    p.join("cgroup.controllers").is_file()
}

fn has_cgroup_children(p: &Path) -> Result<bool> {
    Ok(std::fs::read_dir(p)
        .with_context(|| format!("reading directory {}", p.display()))?
        .filter_map(|e| e.ok())
        .any(|e| {
            let path = e.path();
            path.is_dir() && is_cgroup_dir(&path)
        }))
}

fn kind_from_name(name: &str) -> CgroupKind {
    let suffix = name.rsplit('.').next().unwrap_or("");
    match suffix {
        "slice" => CgroupKind::Slice,
        "service" => CgroupKind::Service,
        "scope" => CgroupKind::Scope,
        "mount" => CgroupKind::Mount,
        "socket" => CgroupKind::Socket,
        other => CgroupKind::Other(other.to_string()),
    }
}
