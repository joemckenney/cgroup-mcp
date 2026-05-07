use anyhow::{bail, Context, Result};
use schemars::JsonSchema;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Pressure {
    pub some: PressureLine,
    pub full: Option<PressureLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct PressureLine {
    pub avg10: f64,
    pub avg60: f64,
    pub avg300: f64,
    pub total_usec: u64,
}

pub fn read_pressure(path: &Path) -> Result<Pressure> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading pressure file at {}", path.display()))?;
    parse_pressure(&raw)
}

pub fn parse_pressure(s: &str) -> Result<Pressure> {
    let mut some = None;
    let mut full = None;

    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let kind = parts.next().context("empty pressure line")?;
        let parsed = parse_pressure_line(parts)?;
        match kind {
            "some" => some = Some(parsed),
            "full" => full = Some(parsed),
            other => bail!("unexpected pressure line kind: {other}"),
        }
    }

    let some = some.context("missing `some` line in pressure file")?;
    Ok(Pressure { some, full })
}

fn parse_pressure_line<'a>(parts: impl Iterator<Item = &'a str>) -> Result<PressureLine> {
    let mut avg10 = None;
    let mut avg60 = None;
    let mut avg300 = None;
    let mut total = None;

    for part in parts {
        let (k, v) = part
            .split_once('=')
            .with_context(|| format!("malformed key=value: {part}"))?;
        match k {
            "avg10" => avg10 = Some(v.parse().with_context(|| format!("avg10={v}"))?),
            "avg60" => avg60 = Some(v.parse().with_context(|| format!("avg60={v}"))?),
            "avg300" => avg300 = Some(v.parse().with_context(|| format!("avg300={v}"))?),
            "total" => total = Some(v.parse().with_context(|| format!("total={v}"))?),
            _ => {} // forward-compat: ignore unknown fields the kernel may add
        }
    }

    Ok(PressureLine {
        avg10: avg10.context("missing avg10")?,
        avg60: avg60.context("missing avg60")?,
        avg300: avg300.context("missing avg300")?,
        total_usec: total.context("missing total")?,
    })
}
