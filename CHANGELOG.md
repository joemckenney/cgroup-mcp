# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/joemckenney/cgroup-mcp/compare/v0.1.0...v0.1.1) - 2026-05-13

### Fixed

- *(release)* set git_only=true so release-plz uses tags as baseline

### Other

- add MIT license
- *(release)* take tag as workflow_dispatch input on release-binaries
- *(release)* use PAT for release-plz and add workflow_dispatch on binaries
- release v0.1.0

## [0.1.0](https://github.com/joemckenney/cgroup-mcp/releases/tag/v0.1.0) - 2026-05-12

### Added

- *(tools)* add top_io MCP tool for disk IO rate ranking
- *(mcp)* add top_cpu tool for sampling CPU usage across cgroups
- *(mcp)* add recent_oom_events tool to query cgroup memory OOM counters
- *(mcp)* add get_unit_stats tool for per-cgroup stat drill-down
- *(mcp)* add top_memory tool to rank cgroups by memory usage
- *(mcp)* wire MCP server with get_pressure tool over stdio
- *(collector)* add ResourceStats bundle for per-cgroup stat collection
- *(collector)* add CPU and IO rate calculation module
- *(collector)* add cgroup stats parsing module with tests
- *(collector)* add cgroup tree walker with snapshot tests
- *(collector)* bootstrap cgroup-mcp with pressure parser and fixtures

### Fixed

- *(release)* drop publish=false from Cargo.toml to unblock release-plz
- *(mcp)* use sort_by_key with Reverse for descending memory sort

### Other

- add binary release workflow and curl installer
- *(release)* configure release-plz for automated GitHub Releases
- *(readme)* backfill shipped tools and trim Next up
- add GitHub Actions workflow for fmt/clippy/test/build
- apply cargo fmt across the crate
- add README with project overview and tool documentation
