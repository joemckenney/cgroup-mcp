use crate::mcp::tools::get_pressure::{self, GetPressureParams, GetPressureResponse};
use crate::mcp::tools::get_unit_stats::{self, GetUnitStatsParams, GetUnitStatsResponse};
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
