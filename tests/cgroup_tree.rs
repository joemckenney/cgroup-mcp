use cgroup_mcp::collector::tree::{read_cgroup_tree, CgroupKind, CgroupNode};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Build a synthetic cgroup tree under a tempdir. Each path becomes a directory
/// containing an empty `cgroup.controllers` file, marking it as a cgroup.
fn synthetic_tree(paths: &[&str]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("cgroup.controllers"), "").unwrap();
    for rel in paths {
        // ensure every component along the way is also a cgroup directory
        let mut current = dir.path().to_path_buf();
        for component in Path::new(rel).components() {
            current.push(component);
            fs::create_dir_all(&current).unwrap();
            let marker = current.join("cgroup.controllers");
            if !marker.exists() {
                fs::write(marker, "").unwrap();
            }
        }
    }
    dir
}

fn real_arch_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/real_arch")
}

fn find_descendant<'a>(node: &'a CgroupNode, rel: &str) -> Option<&'a CgroupNode> {
    if node.relative_path == rel {
        return Some(node);
    }
    for c in &node.children {
        if let Some(n) = find_descendant(c, rel) {
            return Some(n);
        }
    }
    None
}

#[test]
fn rejects_non_cgroup_root() {
    let dir = tempfile::tempdir().unwrap();
    // no cgroup.controllers file
    let err = read_cgroup_tree(dir.path(), None).unwrap_err();
    assert!(
        err.to_string().contains("cgroup.controllers"),
        "error was: {err}"
    );
}

#[test]
fn walks_simple_synthetic_tree() {
    let dir = synthetic_tree(&[
        "system.slice",
        "system.slice/foo.service",
        "user.slice",
    ]);
    let tree = read_cgroup_tree(dir.path(), None).unwrap();
    assert_eq!(tree.kind, CgroupKind::Root);
    assert_eq!(tree.name, "");
    assert_eq!(tree.relative_path, "");
    assert_eq!(tree.children.len(), 2);
    // sorted alphabetically
    assert_eq!(tree.children[0].name, "system.slice");
    assert_eq!(tree.children[0].kind, CgroupKind::Slice);
    assert_eq!(tree.children[1].name, "user.slice");
    let foo = &tree.children[0].children[0];
    assert_eq!(foo.name, "foo.service");
    assert_eq!(foo.kind, CgroupKind::Service);
    assert_eq!(foo.relative_path, "system.slice/foo.service");
}

#[test]
fn skips_non_cgroup_directories() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("cgroup.controllers"), "").unwrap();
    // a real cgroup child
    fs::create_dir(dir.path().join("real.service")).unwrap();
    fs::write(
        dir.path().join("real.service/cgroup.controllers"),
        "",
    )
    .unwrap();
    // a non-cgroup directory (no cgroup.controllers) — should be skipped
    fs::create_dir(dir.path().join("not_a_cgroup")).unwrap();
    fs::write(dir.path().join("not_a_cgroup/random_file"), "").unwrap();

    let tree = read_cgroup_tree(dir.path(), None).unwrap();
    assert_eq!(tree.children.len(), 1);
    assert_eq!(tree.children[0].name, "real.service");
}

#[test]
fn max_depth_zero_returns_root_only_and_marks_truncated() {
    let dir = synthetic_tree(&["system.slice", "system.slice/foo.service"]);
    let tree = read_cgroup_tree(dir.path(), Some(0)).unwrap();
    assert!(tree.children.is_empty());
    assert!(tree.truncated, "root should be marked truncated when it has children");
}

#[test]
fn max_depth_zero_on_leaf_is_not_truncated() {
    // root with no children: depth=0 limit, no truncation should be flagged
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("cgroup.controllers"), "").unwrap();
    let tree = read_cgroup_tree(dir.path(), Some(0)).unwrap();
    assert!(!tree.truncated);
}

#[test]
fn max_depth_one_includes_immediate_children_and_marks_them_truncated() {
    let dir = synthetic_tree(&[
        "system.slice",
        "system.slice/foo.service",
        "user.slice",
    ]);
    let tree = read_cgroup_tree(dir.path(), Some(1)).unwrap();
    assert_eq!(tree.children.len(), 2);
    let sys = &tree.children[0];
    assert_eq!(sys.name, "system.slice");
    assert!(sys.children.is_empty());
    assert!(sys.truncated, "system.slice has children; should be truncated at depth 1");
    let user = &tree.children[1];
    assert!(!user.truncated, "user.slice is empty; should not be truncated");
}

#[test]
fn detects_various_unit_kinds() {
    let dir = synthetic_tree(&[
        "thing.slice",
        "thing.service",
        "thing.scope",
        "thing.mount",
        "thing.socket",
        "thing.timer",
        "no_extension",
    ]);
    let tree = read_cgroup_tree(dir.path(), None).unwrap();
    let kinds: std::collections::HashMap<_, _> = tree
        .children
        .iter()
        .map(|c| (c.name.clone(), c.kind.clone()))
        .collect();

    assert_eq!(kinds["thing.slice"], CgroupKind::Slice);
    assert_eq!(kinds["thing.service"], CgroupKind::Service);
    assert_eq!(kinds["thing.scope"], CgroupKind::Scope);
    assert_eq!(kinds["thing.mount"], CgroupKind::Mount);
    assert_eq!(kinds["thing.socket"], CgroupKind::Socket);
    assert_eq!(kinds["thing.timer"], CgroupKind::Other("timer".into()));
    // a name with no '.' falls through to Other(name) — defensive, not expected in practice
    assert_eq!(
        kinds["no_extension"],
        CgroupKind::Other("no_extension".into())
    );
}

#[test]
fn handles_systemd_escaped_names() {
    // systemd escapes '-' as \x2d in cgroup names: e.g. system-systemd\x2dcoredump.slice
    let dir = synthetic_tree(&["system.slice/system-systemd\\x2dcoredump.slice"]);
    let tree = read_cgroup_tree(dir.path(), None).unwrap();
    let escaped = &tree.children[0].children[0];
    assert_eq!(escaped.name, "system-systemd\\x2dcoredump.slice");
    assert_eq!(escaped.kind, CgroupKind::Slice);
}

#[test]
fn walks_real_arch_capture() {
    let tree = read_cgroup_tree(&real_arch_root(), None).unwrap();
    assert_eq!(tree.kind, CgroupKind::Root);

    let sys = find_descendant(&tree, "system.slice").expect("system.slice present");
    assert_eq!(sys.kind, CgroupKind::Slice);

    for unit in [
        "system.slice/dbus-broker.service",
        "system.slice/systemd-journald.service",
        "system.slice/NetworkManager.service",
    ] {
        let n = find_descendant(&tree, unit).unwrap_or_else(|| panic!("missing: {unit}"));
        assert_eq!(n.kind, CgroupKind::Service, "{unit} should be a service");
    }

    let nested = find_descendant(&tree, "system.slice/system-getty.slice")
        .expect("nested slice present");
    assert_eq!(nested.kind, CgroupKind::Slice);
}

#[test]
fn real_arch_snapshot() {
    let tree = read_cgroup_tree(&real_arch_root(), None).unwrap();
    insta::assert_yaml_snapshot!(tree);
}

#[test]
fn max_depth_against_real_arch() {
    // depth=1 should give us just the slice level under the root, with each
    // marked truncated if it has children.
    let tree = read_cgroup_tree(&real_arch_root(), Some(1)).unwrap();
    let sys = tree
        .children
        .iter()
        .find(|c| c.name == "system.slice")
        .expect("system.slice");
    assert!(sys.children.is_empty());
    assert!(sys.truncated, "system.slice has captured services beneath it");
}
