# cgroup-mcp

A read-only MCP server that exposes Linux cgroup v2 state (resource usage, PSI pressure, OOM counters) as structured tool calls for AI agents.

## What it does

Linux exposes detailed per-cgroup resource accounting under `/sys/fs/cgroup`: memory, CPU, and IO usage per slice/service/scope, Pressure Stall Information (PSI), and OOM event counters. Tools like `btop`, `systemd-cgtop`, and `htop` read this for humans. This server makes the same data available to AI agents as structured tool calls, so an agent can answer "what's using memory," "is anything stalled waiting on IO," or "did anything get killed recently" with concrete data instead of inference.

PSI is worth calling out specifically because it isn't widely surfaced for agents. It reports the percentage of time tasks were stalled on a resource over rolling 10s/60s/300s windows, distinguishing "the box is busy and working" from "the box is busy and struggling." Conventional process viewers show usage but not waiting; PSI is the structured signal for the second question.

## Status

Early. Two tools shipped so far. The collector layer (tree walking, stat parsing, rate math) is complete and tested; the MCP layer is wired over stdio with the first two tools.

## Planned tools

Driven by the v1 plan, sequenced by dogfooding feedback.

Shipped:

- `get_pressure`: PSI (memory, CPU, IO) for a specified cgroup or system-wide
- `top_memory`: top memory consumers under a subtree

Next up:

- `top_cpu`: top CPU consumers, computed from snapshot deltas
- `top_io`: top IO consumers per block device, also delta-based
- `top_pressure`: cgroups sorted by stall percentage on a chosen resource
- `list_cgroups`: slice/service/scope hierarchy at configurable depth
- `get_unit_stats`: full stat bundle for a named systemd unit
- `recent_oom_events`: cgroups that have OOM-killed processes since boot, with counts from `memory.events`
- `system_summary`: composed snapshot answering "what's happening on this box"

Deliberately deferred (see Design notes below): cross-machine federation, any write paths, LLM summarization inside the server.

## Requirements

- Linux with cgroup v2 unified hierarchy. Default on Arch, Fedora 31+, Ubuntu 21.10+, Debian 12+, RHEL 9+, and recent container distros.
- Kernel 4.20 or newer for PSI.
- Rust toolchain to build.

Does not work on macOS, Windows, or BSD. Cgroups are a Linux kernel feature with no equivalent elsewhere.

## Build

```
cargo build --release
```

Binary is at `./target/release/cgroup-mcp`.

## Use with Claude Code

Add to your MCP config:

```json
{
  "mcpServers": {
    "cgroup": {
      "command": "/absolute/path/to/cgroup-mcp"
    }
  }
}
```

The cgroup root defaults to `/sys/fs/cgroup`. Override with `--cgroup-root <path>` if you need to point at a different mount.

## Tools

### get_pressure

Returns PSI for a cgroup over rolling 10s/60s/300s windows. The `some` stanza means at least one task was stalled; `full` (memory and IO only) means all non-idle tasks were stalled. Pass an empty path for the system-wide root cgroup.

### top_memory

Returns the cgroups using the most memory under a given subtree, sorted descending by `memory.current` bytes. Slices and the root cgroup are excluded because their `memory.current` reflects summed descendant memory rather than actual leaf consumption. Default `n` is 10.

## Tests

```
cargo test
```

Tests cover the collector layer (parsers, tree walker, rate calculation) and the MCP layer (tool schemas, behavior against captured fixtures). The `tests/fixtures/real_arch/` directory holds a sanitized capture from a live cgroup tree, used for snapshot tests and a smoke test that verifies every parser handles real kernel output.

## Design notes

Three-layer architecture: a pure-function collector that reads `/sys/fs/cgroup` and returns typed structs, a thin MCP wrapper that exposes collector output as tools, and the transport (stdio for local use). The collector has no MCP dependency and could be reused as a library.

Read-only by intent. No write paths in v1, no `kill_pid`, no `change_unit_state`. Mixing read and write is the failure mode that bites every system tool, and the security story is dramatically simpler without it.

Snapshot, not stream. Each tool call is a point-in-time read. For time-series, the agent takes multiple snapshots and reasons about deltas. CPU and IO rates use a snapshot-sleep-snapshot pattern internal to the rate-based tools.
