use crate::collector::stats::{CpuStat, IoDeviceStat};
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CpuRate {
    /// Fraction of wall time the cgroup was on CPU during the window.
    /// 1.0 = one full core saturated; on an N-core system this can range 0.0..N.
    pub usage_ratio: f64,
    pub user_ratio: f64,
    pub system_ratio: f64,
    pub usage_delta_usec: u64,
    /// True if any CPU counter went backwards between snapshots — typically
    /// a cgroup recreation (service restart). All `_ratio` fields are 0.0
    /// when this is set.
    pub reset_detected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct IoRate {
    pub major: u32,
    pub minor: u32,
    pub rbytes_per_sec: f64,
    pub wbytes_per_sec: f64,
    pub rios_per_sec: f64,
    pub wios_per_sec: f64,
    pub dbytes_per_sec: f64,
    pub dios_per_sec: f64,
    /// Any counter on this device went backwards. Rates are 0.0 in that case.
    pub reset_detected: bool,
}

pub fn cpu_rate(a: &CpuStat, b: &CpuStat, dt: Duration) -> Result<CpuRate> {
    if dt.is_zero() {
        bail!("cannot compute CPU rate over zero duration");
    }
    let dt_usec = dt.as_secs_f64() * 1_000_000.0;

    let (usage_delta, reset_u) = monotonic_delta(a.usage_usec, b.usage_usec);
    let (user_delta, reset_user) = monotonic_delta(a.user_usec, b.user_usec);
    let (system_delta, reset_sys) = monotonic_delta(a.system_usec, b.system_usec);
    let reset = reset_u || reset_user || reset_sys;

    let ratio = |usec: u64| if reset { 0.0 } else { usec as f64 / dt_usec };

    Ok(CpuRate {
        usage_ratio: ratio(usage_delta),
        user_ratio: ratio(user_delta),
        system_ratio: ratio(system_delta),
        usage_delta_usec: usage_delta,
        reset_detected: reset,
    })
}

pub fn io_rates(a: &[IoDeviceStat], b: &[IoDeviceStat], dt: Duration) -> Result<Vec<IoRate>> {
    if dt.is_zero() {
        bail!("cannot compute IO rate over zero duration");
    }
    let dt_secs = dt.as_secs_f64();

    let prev_by_dev: HashMap<(u32, u32), &IoDeviceStat> =
        a.iter().map(|d| ((d.major, d.minor), d)).collect();

    let mut out = Vec::with_capacity(b.len());
    for cur in b {
        let key = (cur.major, cur.minor);
        // Devices that didn't exist in `a` get skipped — we have no prior
        // counter, so a rate isn't computable.
        let Some(prev) = prev_by_dev.get(&key) else {
            continue;
        };

        let (rbytes_d, r1) = monotonic_delta(prev.rbytes, cur.rbytes);
        let (wbytes_d, r2) = monotonic_delta(prev.wbytes, cur.wbytes);
        let (rios_d, r3) = monotonic_delta(prev.rios, cur.rios);
        let (wios_d, r4) = monotonic_delta(prev.wios, cur.wios);
        let (dbytes_d, r5) = monotonic_delta(prev.dbytes, cur.dbytes);
        let (dios_d, r6) = monotonic_delta(prev.dios, cur.dios);
        let reset = r1 || r2 || r3 || r4 || r5 || r6;

        let rate = |delta: u64| if reset { 0.0 } else { delta as f64 / dt_secs };

        out.push(IoRate {
            major: cur.major,
            minor: cur.minor,
            rbytes_per_sec: rate(rbytes_d),
            wbytes_per_sec: rate(wbytes_d),
            rios_per_sec: rate(rios_d),
            wios_per_sec: rate(wios_d),
            dbytes_per_sec: rate(dbytes_d),
            dios_per_sec: rate(dios_d),
            reset_detected: reset,
        });
    }
    out.sort_by_key(|r| (r.major, r.minor));
    Ok(out)
}

fn monotonic_delta(a: u64, b: u64) -> (u64, bool) {
    if b >= a {
        (b - a, false)
    } else {
        (0, true)
    }
}
