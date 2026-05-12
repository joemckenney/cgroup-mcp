# cgroup-mcp

A read-only MCP server exposing Linux cgroup v2 state (resource accounting, PSI, OOM events) as structured tools for AI agents.

## Installation

```sh
curl -sSf https://raw.githubusercontent.com/joemckenney/cgroup-mcp/main/install.sh | sh
```

Linux only. Pre-built binaries for `x86_64` and `aarch64`.

## Setup

Add the MCP server to Claude Code:

```sh
claude mcp add --transport stdio --scope user cgroup -- cgroup-mcp
```

The cgroup root defaults to `/sys/fs/cgroup`. Override with `--cgroup-root <path>` if needed (useful for testing against captured trees).

## Usage

Ask Claude questions about resource use on the host. It picks the right tool from the catalog below.

```
> What's using the most memory on this box?
```

Claude calls `top_memory`, gets back a ranked list of cgroups by `memory.current`, and answers with concrete numbers instead of guessing. From there it can drill in with `get_unit_stats` for a single cgroup, or check `recent_oom_events` to see if anything has been killed.

```
> Is anything stalled on memory pressure right now?
```

Claude calls `get_pressure` for the cgroups likely to be under load, reads PSI's rolling 10s/60s/300s windows, and answers with "yes, X has Y% memory stall over the last minute" or "nothing's stalled."

## Tools

| Tool                 | Purpose                                                                   |
| -------------------- | ------------------------------------------------------------------------- |
| `get_pressure`       | PSI (memory, CPU, IO) for a cgroup or system-wide                         |
| `top_memory`         | Top memory consumers under a subtree, sorted by `memory.current`          |
| `top_cpu`            | Top CPU consumers (sampled rate over a window, includes throttle deltas)  |
| `top_io`             | Top IO consumers (sampled rate, aggregate + per-device breakdown)         |
| `get_unit_stats`     | Full stat bundle for one cgroup, grouped into cpu/memory/io sections      |
| `recent_oom_events`  | Cgroups whose `memory.events.local` has any non-zero counter              |

`top_cpu` and `top_io` block for the duration of the sampling window (default 500ms, configurable per call) because rates require two reads with time between them. The other tools return instantly.

## Requirements

- Linux with cgroup v2 unified hierarchy. Default on Arch, Fedora 31+, Ubuntu 21.10+, Debian 12+, RHEL 9+, recent container distros.
- Kernel 4.20 or newer for PSI.

Does not run on macOS, Windows, or BSD. Cgroups are a Linux kernel feature with no equivalent elsewhere. Per-process drill-down (PIDs inside a cgroup, top processes by RSS) lives in a separate sister project, `process-mcp`.

## How It Works

```
┌──────────────────────────────────────────────────────────────────────┐
│                              cgroup-mcp                              │
│                                                                      │
│  ┌─────────────┐   parse    ┌──────────────┐   wrap   ┌───────────┐  │
│  │  /sys/fs/   │──────────▶ │  Collector   │────────▶ │   MCP     │  │
│  │  cgroup/*   │   typed    │  (pure fns)  │  tools   │  Server   │  │
│  └─────────────┘   structs  └──────────────┘          └─────┬─────┘  │
│                                                             │ stdio  │
└─────────────────────────────────────────────────────────────┼────────┘
                                                              │
                                                              ▼
                                                       ┌─────────────┐
                                                       │   Claude    │
                                                       │    Code     │
                                                       └─────────────┘
```

Three layers: a pure-function collector that reads `/sys/fs/cgroup` and returns typed Rust structs, a thin MCP wrapper that exposes collector output as tools, and stdio transport. Each tool call is a point-in-time snapshot. For time-series, the agent takes multiple snapshots and reasons about deltas. CPU and IO rates use an internal snapshot-sleep-snapshot pattern.

Read-only by design. No write paths, no `kill_pid`, no `change_unit_state`.

## Releases

Driven by [release-plz](https://release-plz.dev) reading [conventional commits](https://www.conventionalcommits.org/). On push to `main`, the workflow inspects commits since the last `v*` tag. If any imply a version bump (`feat:` for minor, `fix:` for patch, `feat!:` or `BREAKING CHANGE:` for major; pre-1.0, breaking changes bump minor), it opens a `chore: release vX.Y.Z` PR with version + changelog. Merging that PR tags the commit and triggers the binary workflow, which builds `x86_64` and `aarch64` tarballs and uploads them to the GitHub Release.

## Building from Source

```sh
git clone https://github.com/joemckenney/cgroup-mcp
cd cgroup-mcp
cargo build --release
# binary at ./target/release/cgroup-mcp
```

## Tests

```sh
cargo test
```

Tests cover the collector layer (parsers, tree walker, rate calculation) and the MCP layer (tool schemas, behavior against captured fixtures). The `tests/fixtures/real_arch/` directory holds a sanitized capture from a live cgroup tree.
