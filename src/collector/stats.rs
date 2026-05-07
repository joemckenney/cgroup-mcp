use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct CpuStat {
    pub usage_usec: u64,
    pub user_usec: u64,
    pub system_usec: u64,
    pub nice_usec: u64,
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_usec: u64,
    pub nr_bursts: u64,
    pub burst_usec: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct MemoryStat {
    pub raw: BTreeMap<String, u64>,
}

impl MemoryStat {
    /// Convenience accessor; returns 0 if the field is absent. The `raw` map
    /// holds the full set so callers can introspect uncommon fields.
    pub fn get(&self, key: &str) -> u64 {
        self.raw.get(key).copied().unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct MemoryEvents {
    pub low: u64,
    pub high: u64,
    pub max: u64,
    pub oom: u64,
    pub oom_kill: u64,
    pub oom_group_kill: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IoDeviceStat {
    pub major: u32,
    pub minor: u32,
    pub rbytes: u64,
    pub wbytes: u64,
    pub rios: u64,
    pub wios: u64,
    pub dbytes: u64,
    pub dios: u64,
}

pub fn read_cpu_stat(path: &Path) -> Result<CpuStat> {
    parse_cpu_stat(&read_file(path)?)
}

pub fn read_memory_current(path: &Path) -> Result<u64> {
    parse_memory_current(&read_file(path)?)
}

pub fn read_memory_stat(path: &Path) -> Result<MemoryStat> {
    parse_memory_stat(&read_file(path)?)
}

pub fn read_memory_events(path: &Path) -> Result<MemoryEvents> {
    parse_memory_events(&read_file(path)?)
}

pub fn read_io_stat(path: &Path) -> Result<Vec<IoDeviceStat>> {
    parse_io_stat(&read_file(path)?)
}

pub fn parse_cpu_stat(s: &str) -> Result<CpuStat> {
    let m = parse_simple_kv(s).context("parsing cpu.stat")?;
    Ok(CpuStat {
        usage_usec: m.get("usage_usec").copied().unwrap_or(0),
        user_usec: m.get("user_usec").copied().unwrap_or(0),
        system_usec: m.get("system_usec").copied().unwrap_or(0),
        nice_usec: m.get("nice_usec").copied().unwrap_or(0),
        nr_periods: m.get("nr_periods").copied().unwrap_or(0),
        nr_throttled: m.get("nr_throttled").copied().unwrap_or(0),
        throttled_usec: m.get("throttled_usec").copied().unwrap_or(0),
        nr_bursts: m.get("nr_bursts").copied().unwrap_or(0),
        burst_usec: m.get("burst_usec").copied().unwrap_or(0),
    })
}

pub fn parse_memory_current(s: &str) -> Result<u64> {
    let trimmed = s.trim();
    trimmed
        .parse()
        .with_context(|| format!("parsing memory.current: {trimmed:?}"))
}

pub fn parse_memory_stat(s: &str) -> Result<MemoryStat> {
    Ok(MemoryStat {
        raw: parse_simple_kv(s).context("parsing memory.stat")?,
    })
}

pub fn parse_memory_events(s: &str) -> Result<MemoryEvents> {
    let m = parse_simple_kv(s).context("parsing memory.events")?;
    Ok(MemoryEvents {
        low: m.get("low").copied().unwrap_or(0),
        high: m.get("high").copied().unwrap_or(0),
        max: m.get("max").copied().unwrap_or(0),
        oom: m.get("oom").copied().unwrap_or(0),
        oom_kill: m.get("oom_kill").copied().unwrap_or(0),
        oom_group_kill: m.get("oom_group_kill").copied().unwrap_or(0),
    })
}

pub fn parse_io_stat(s: &str) -> Result<Vec<IoDeviceStat>> {
    let mut devices = Vec::new();
    for (lineno, raw_line) in s.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        devices.push(
            parse_io_stat_line(line)
                .with_context(|| format!("io.stat line {}: {raw_line:?}", lineno + 1))?,
        );
    }
    Ok(devices)
}

fn parse_io_stat_line(line: &str) -> Result<IoDeviceStat> {
    let mut parts = line.split_whitespace();
    let device = parts.next().context("missing device prefix")?;
    let (major, minor) = device
        .split_once(':')
        .with_context(|| format!("device must be major:minor, got {device:?}"))?;
    let major: u32 = major
        .parse()
        .with_context(|| format!("parsing major from {device:?}"))?;
    let minor: u32 = minor
        .parse()
        .with_context(|| format!("parsing minor from {device:?}"))?;

    let mut kv: BTreeMap<&str, u64> = BTreeMap::new();
    for part in parts {
        let (k, v) = part
            .split_once('=')
            .with_context(|| format!("malformed key=value: {part:?}"))?;
        let v: u64 = v
            .parse()
            .with_context(|| format!("parsing value for {k}={v}"))?;
        kv.insert(k, v);
    }

    Ok(IoDeviceStat {
        major,
        minor,
        rbytes: kv.get("rbytes").copied().unwrap_or(0),
        wbytes: kv.get("wbytes").copied().unwrap_or(0),
        rios: kv.get("rios").copied().unwrap_or(0),
        wios: kv.get("wios").copied().unwrap_or(0),
        dbytes: kv.get("dbytes").copied().unwrap_or(0),
        dios: kv.get("dios").copied().unwrap_or(0),
    })
}

fn parse_simple_kv(s: &str) -> Result<BTreeMap<String, u64>> {
    let mut map = BTreeMap::new();
    for (lineno, raw_line) in s.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let key = parts
            .next()
            .with_context(|| format!("line {}: empty after trim", lineno + 1))?;
        let value_str = parts
            .next()
            .with_context(|| format!("line {}: missing value for {key:?}", lineno + 1))?;
        let value: u64 = value_str.parse().with_context(|| {
            format!(
                "line {}: parsing value for {key:?}: {value_str:?}",
                lineno + 1
            )
        })?;
        map.insert(key.to_string(), value);
    }
    Ok(map)
}

fn read_file(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}
