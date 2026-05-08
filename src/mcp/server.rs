use crate::mcp::tools::get_pressure::{self, GetPressureParams, GetPressureResponse};
use crate::mcp::tools::get_unit_stats::{self, GetUnitStatsParams, GetUnitStatsResponse};
use crate::mcp::tools::recent_oom_events::{self, RecentOomEventsParams, RecentOomEventsResponse};
use crate::mcp::tools::top_cpu::{self, TopCpuParams, TopCpuResponse};
use crate::mcp::tools::top_io::{self, TopIoParams, TopIoResponse};
use crate::mcp::tools::top_memory::{self, TopMemoryParams, TopMemoryResponse};
use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, Json, ServerHandler};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CgroupServer {
    cgroup_root: PathBuf,
    tool_router: ToolRouter<Self>,
}

impl CgroupServer {
    pub fn new(cgroup_root: PathBuf) -> Self {
        Self {
            cgroup_root,
            tool_router: Self::tool_router(),
        }
    }

    /// Tool catalog as it would be returned from `tools/list`. Used by tests
    /// to snapshot the public schema surface.
    pub fn list_tools(&self) -> Vec<rmcp::model::Tool> {
        self.tool_router.list_all()
    }
}

#[tool_router(router = tool_router)]
impl CgroupServer {
    /// Returns pressure stall information (PSI) for a specific cgroup or
    /// system-wide. PSI shows the percentage of time tasks were stalled
    /// waiting for a resource over rolling 10s/60s/300s windows. Use this
    /// to distinguish "the system is busy" (high CPU usage, no waiting)
    /// from "the system is struggling" (high stall percentages). The
    /// `some` line means at least one task was stalled; the `full` line
    /// (memory and IO only) means all non-idle tasks were stalled. Pass an
    /// empty path string for system-wide pressure.
    #[tool(
        name = "get_pressure",
        description = "Returns pressure stall information (PSI) for a cgroup or system-wide. \
            PSI gives the percentage of time tasks were stalled waiting for memory, CPU, \
            or IO over 10s/60s/300s rolling windows — distinguishing 'busy' from \
            'struggling.' Pass an empty path for the root (system-wide) cgroup, or a \
            relative path like 'system.slice/nginx.service'. Returns three pressure \
            stanzas (memory, cpu, io); each may be null if the controller is not \
            enabled on the targeted cgroup."
    )]
    pub async fn get_pressure(
        &self,
        Parameters(params): Parameters<GetPressureParams>,
    ) -> Result<Json<GetPressureResponse>, McpError> {
        get_pressure::run(&self.cgroup_root, params)
            .map(Json)
            .map_err(|e| McpError::internal_error(format!("{e:#}"), None))
    }

    /// Returns the cgroups using the most memory under a subtree, sorted
    /// descending. Memory.current is a gauge — no rate computation. Slices
    /// and the root cgroup are excluded since their memory.current is the
    /// sum of their descendants and would dominate the ranking.
    #[tool(
        name = "top_memory",
        description = "Returns the cgroups using the most memory right now under a given \
            subtree, sorted descending by memory.current bytes. Use this to answer \
            'what's using the most memory.' Pass an empty path for the whole tree, or a \
            relative path like 'system.slice' or 'user.slice' to scope the search. \
            Slices and the root cgroup are excluded from results because their \
            memory.current shows summed descendant memory, not actual leaf consumers. \
            Default n is 10."
    )]
    pub async fn top_memory(
        &self,
        Parameters(params): Parameters<TopMemoryParams>,
    ) -> Result<Json<TopMemoryResponse>, McpError> {
        top_memory::run(&self.cgroup_root, params)
            .map(Json)
            .map_err(|e| McpError::internal_error(format!("{e:#}"), None))
    }

    /// Returns the full set of cgroup v2 stat files for a single cgroup,
    /// grouped by controller. Use this once you've identified a cgroup of
    /// interest (e.g. via top_memory) to drill into what it's actually
    /// doing — memory breakdown by anon/file/slab, OOM event counts, CPU
    /// time and throttling, IO counters, and pressure.
    #[tool(
        name = "get_unit_stats",
        description = "Returns the full set of cgroup v2 stat files for a single cgroup, \
            grouped into cpu, memory, and io sections. Each section contains the relevant \
            stat counters and the pressure (PSI) for that controller. Use this to drill \
            into a specific cgroup once you've identified it (e.g. via top_memory) — it \
            answers 'what is this cgroup actually doing right now?' Memory.stat is \
            returned as a raw key/value map so all kernel fields are available (anon, \
            file, slab, kernel, sock, shmem, pgfault, oom_kill, etc.). Pass an empty path \
            for the root cgroup, or a relative path like 'system.slice/nginx.service'. \
            Individual fields are null when the corresponding file is absent (e.g. some \
            cgroups don't expose memory.current; root cgroups vary by kernel)."
    )]
    pub async fn get_unit_stats(
        &self,
        Parameters(params): Parameters<GetUnitStatsParams>,
    ) -> Result<Json<GetUnitStatsResponse>, McpError> {
        get_unit_stats::run(&self.cgroup_root, params)
            .map(Json)
            .map_err(|e| McpError::internal_error(format!("{e:#}"), None))
    }

    /// Walks a cgroup subtree and returns every cgroup whose
    /// memory.events.local has any non-zero counter. Counters are
    /// CUMULATIVE since the cgroup was created — there is no timestamp,
    /// so a non-zero count means "this happened at some point," not
    /// "this happened recently."
    #[tool(
        name = "recent_oom_events",
        description = "Walks a cgroup subtree and returns every cgroup whose \
            memory.events.local has at least one non-zero counter (low, high, max, oom, \
            oom_kill, oom_group_kill). Reads .local rather than memory.events so a slice \
            doesn't appear OOMed when the actual kill happened in a child. \
            \
            IMPORTANT: counters are CUMULATIVE since the cgroup was created — they have \
            no timestamp and no rolling window. A non-zero count means 'this happened at \
            some point,' not 'this happened recently.' Do not tell the user something \
            'just' OOMed based on this output. \
            \
            Pass an empty path for the whole tree, or a relative path like 'system.slice' \
            to scope the search. By default returns only cgroups with at least one \
            non-zero counter; pass include_zero=true to confirm 'nothing has OOMed.' \
            Results are sorted by oom_kill desc, then oom desc."
    )]
    pub async fn recent_oom_events(
        &self,
        Parameters(params): Parameters<RecentOomEventsParams>,
    ) -> Result<Json<RecentOomEventsResponse>, McpError> {
        recent_oom_events::run(&self.cgroup_root, params)
            .map(Json)
            .map_err(|e| McpError::internal_error(format!("{e:#}"), None))
    }

    /// Returns the cgroups using the most CPU under a subtree, sorted
    /// descending by CPU time consumed during a sampling window. Unlike
    /// memory, CPU usage only makes sense as a rate, so this tool blocks
    /// for sample_window_ms (default 500ms) reading cpu.stat before and
    /// after. Slices and root are excluded since their cpu.stat is summed
    /// across descendants.
    #[tool(
        name = "top_cpu",
        description = "Returns the cgroups using the most CPU under a given subtree, sorted \
            descending by CPU time consumed during a sampling window. Use this to answer \
            'what's using the most CPU.' \
            \
            UNLIKE other tools in this server, this one BLOCKS for the duration of the \
            sample window (default 500ms) — it reads cpu.stat once, sleeps, and reads \
            again to compute a rate. Shorter windows respond faster but become noisy \
            below ~50ms; longer windows are more stable but slower. \
            \
            Each entry returns CPU consumption as both `usage_cores` (1.0 = one full core \
            for the whole window) and `usage_delta_usec` (raw microseconds). Throttle \
            counters are also returned per entry — non-zero `throttled_periods_delta` \
            means the cgroup wanted more CPU but hit its limit. Pass an empty path for \
            the whole tree, or a relative path like 'system.slice'. Slices and the root \
            cgroup are excluded from results because their cpu.stat shows summed \
            descendant time, not actual leaf consumers. Default n is 10."
    )]
    pub async fn top_cpu(
        &self,
        Parameters(params): Parameters<TopCpuParams>,
    ) -> Result<Json<TopCpuResponse>, McpError> {
        top_cpu::run(&self.cgroup_root, params)
            .await
            .map(Json)
            .map_err(|e| McpError::internal_error(format!("{e:#}"), None))
    }

    /// Returns the cgroups doing the most disk IO under a subtree, sorted
    /// descending by total bytes/sec consumed during a sampling window.
    /// Like top_cpu, this tool blocks for the duration of the window
    /// because IO usage only makes sense as a rate. Per-device breakdown
    /// is included alongside aggregate totals so the agent can spot a
    /// cgroup hammering one disk vs distributing load.
    #[tool(
        name = "top_io",
        description = "Returns the cgroups doing the most disk IO under a given subtree, sorted \
            descending by total bytes/sec (read + write) consumed during a sampling window. \
            Use this to answer 'what's hammering disk.' \
            \
            Like top_cpu, this tool BLOCKS for the duration of the sample window \
            (default 500ms): it reads io.stat once, sleeps, and reads again to compute \
            rates. Same tradeoff: shorter windows respond faster but become noisy on \
            bursty IO; longer windows are more stable. \
            \
            Each entry returns aggregate rates (total_bytes_per_sec, rbytes_per_sec, \
            wbytes_per_sec, total_ios_per_sec, rios_per_sec, wios_per_sec) summed across \
            all block devices, plus a per_device array for cases where one cgroup is \
            hammering one disk but quiet elsewhere. Pass an empty path for the whole \
            tree, or a relative path like 'system.slice'. Slices and the root cgroup are \
            excluded from results because their io.stat is summed across descendants. \
            Default n is 10. Cgroups without an enabled IO controller are silently \
            skipped."
    )]
    pub async fn top_io(
        &self,
        Parameters(params): Parameters<TopIoParams>,
    ) -> Result<Json<TopIoResponse>, McpError> {
        top_io::run(&self.cgroup_root, params)
            .await
            .map(Json)
            .map_err(|e| McpError::internal_error(format!("{e:#}"), None))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for CgroupServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Read-only access to Linux cgroup v2 state (resource accounting and PSI). \
                 Tools return structured JSON; the agent does prose. Each call is a \
                 point-in-time snapshot.",
        )
    }
}
