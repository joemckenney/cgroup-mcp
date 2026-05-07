use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

/// Resolve a tool's user-supplied relative path against the configured
/// cgroup root. Rejects absolute paths and `..` traversal. The empty
/// string resolves to the root itself.
pub fn resolve_cgroup_dir(cgroup_root: &Path, rel: &str) -> Result<PathBuf> {
    if rel.starts_with('/') {
        bail!("path must be relative to the cgroup root, got absolute: {rel:?}");
    }
    if rel.split('/').any(|seg| seg == "..") {
        bail!("path must not contain `..` segments: {rel:?}");
    }
    let dir = if rel.is_empty() {
        cgroup_root.to_path_buf()
    } else {
        cgroup_root.join(rel)
    };
    if !dir.is_dir() {
        bail!("cgroup not found: {}", dir.display());
    }
    Ok(dir)
}
