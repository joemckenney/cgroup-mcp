use anyhow::{Context, Result};
use cgroup_mcp::mcp::server::CgroupServer;
use rmcp::{transport::stdio, ServiceExt};
use std::path::PathBuf;

const DEFAULT_CGROUP_ROOT: &str = "/sys/fs/cgroup";

#[tokio::main]
async fn main() -> Result<()> {
    let cgroup_root = parse_args()?;
    let service = CgroupServer::new(cgroup_root)
        .serve(stdio())
        .await
        .context("starting MCP service over stdio")?;
    service.waiting().await.context("running MCP service")?;
    Ok(())
}

fn parse_args() -> Result<PathBuf> {
    let mut args = std::env::args().skip(1);
    let mut cgroup_root: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cgroup-root" => {
                let v = args
                    .next()
                    .context("--cgroup-root requires a path argument")?;
                cgroup_root = Some(PathBuf::from(v));
            }
            "--help" | "-h" => {
                eprintln!("cgroup-mcp [--cgroup-root <path>]");
                eprintln!();
                eprintln!("  --cgroup-root  cgroup v2 root (default: {DEFAULT_CGROUP_ROOT})");
                std::process::exit(0);
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    Ok(cgroup_root.unwrap_or_else(|| PathBuf::from(DEFAULT_CGROUP_ROOT)))
}
