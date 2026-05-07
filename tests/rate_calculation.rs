use cgroup_mcp::collector::rate::{cpu_rate, io_rates};
use cgroup_mcp::collector::stats::{CpuStat, IoDeviceStat};
use std::time::Duration;

const EPS: f64 = 1e-9;

fn approx(a: f64, b: f64) {
    assert!(
        (a - b).abs() < EPS,
        "expected {b}, got {a} (diff {})",
        (a - b).abs()
    );
}

fn cpu(usage: u64, user: u64, system: u64) -> CpuStat {
    CpuStat {
        usage_usec: usage,
        user_usec: user,
        system_usec: system,
        ..Default::default()
    }
}

fn iodev(maj: u32, min: u32, r: u64, w: u64) -> IoDeviceStat {
    IoDeviceStat {
        major: maj,
        minor: min,
        rbytes: r,
        wbytes: w,
        rios: 0,
        wios: 0,
        dbytes: 0,
        dios: 0,
    }
}

// ---- CPU ----

#[test]
fn cpu_one_full_core_in_one_second_is_ratio_one() {
    let a = cpu(0, 0, 0);
    let b = cpu(1_000_000, 600_000, 400_000);
    let r = cpu_rate(&a, &b, Duration::from_secs(1)).unwrap();
    approx(r.usage_ratio, 1.0);
    approx(r.user_ratio, 0.6);
    approx(r.system_ratio, 0.4);
    assert_eq!(r.usage_delta_usec, 1_000_000);
    assert!(!r.reset_detected);
}

#[test]
fn cpu_four_cores_saturated_yields_ratio_four() {
    // Multi-threaded workload: 4 cores * 1s = 4_000_000 usec of CPU time
    // accumulated in 1s of wall-clock.
    let a = cpu(0, 0, 0);
    let b = cpu(4_000_000, 4_000_000, 0);
    let r = cpu_rate(&a, &b, Duration::from_secs(1)).unwrap();
    approx(r.usage_ratio, 4.0);
    approx(r.user_ratio, 4.0);
    approx(r.system_ratio, 0.0);
}

#[test]
fn cpu_half_second_window_scales_correctly() {
    let a = cpu(0, 0, 0);
    let b = cpu(500_000, 0, 0);
    let r = cpu_rate(&a, &b, Duration::from_millis(500)).unwrap();
    approx(r.usage_ratio, 1.0);
}

#[test]
fn cpu_idle_cgroup_yields_zero_rate() {
    let a = cpu(123_456, 100_000, 23_456);
    let b = a.clone();
    let r = cpu_rate(&a, &b, Duration::from_secs(1)).unwrap();
    approx(r.usage_ratio, 0.0);
    assert_eq!(r.usage_delta_usec, 0);
    assert!(!r.reset_detected);
}

#[test]
fn cpu_zero_dt_is_an_error() {
    let err = cpu_rate(&cpu(0, 0, 0), &cpu(1, 0, 0), Duration::ZERO).unwrap_err();
    assert!(err.to_string().contains("zero duration"));
}

#[test]
fn cpu_counter_regression_sets_reset_and_zero_rates() {
    // Cgroup recreated between snapshots: counters reset to 0.
    let a = cpu(1_000_000, 600_000, 400_000);
    let b = cpu(50_000, 30_000, 20_000);
    let r = cpu_rate(&a, &b, Duration::from_secs(1)).unwrap();
    assert!(r.reset_detected);
    approx(r.usage_ratio, 0.0);
    approx(r.user_ratio, 0.0);
    approx(r.system_ratio, 0.0);
    assert_eq!(r.usage_delta_usec, 0);
}

#[test]
fn cpu_partial_regression_still_flags_reset() {
    // Edge case: total usage went up but user counter went backwards.
    // Shouldn't happen in practice, but we should detect inconsistency
    // and not silently report a misleading user_ratio.
    let a = cpu(1000, 800, 200);
    let b = cpu(2000, 500, 1500);
    let r = cpu_rate(&a, &b, Duration::from_secs(1)).unwrap();
    assert!(r.reset_detected);
    approx(r.user_ratio, 0.0);
}

// ---- IO ----

#[test]
fn io_simple_per_device_rates() {
    let a = vec![iodev(8, 0, 0, 0), iodev(259, 0, 1_000_000, 500_000)];
    let b = vec![
        iodev(8, 0, 1_048_576, 0),
        iodev(259, 0, 2_000_000, 1_500_000),
    ];
    let r = io_rates(&a, &b, Duration::from_secs(1)).unwrap();
    assert_eq!(r.len(), 2);
    // sorted by (major, minor) -> 8:0, 259:0
    assert_eq!((r[0].major, r[0].minor), (8, 0));
    approx(r[0].rbytes_per_sec, 1_048_576.0);
    approx(r[0].wbytes_per_sec, 0.0);
    assert_eq!((r[1].major, r[1].minor), (259, 0));
    approx(r[1].rbytes_per_sec, 1_000_000.0);
    approx(r[1].wbytes_per_sec, 1_000_000.0);
}

#[test]
fn io_zero_dt_is_an_error() {
    let err = io_rates(&[], &[], Duration::ZERO).unwrap_err();
    assert!(err.to_string().contains("zero duration"));
}

#[test]
fn io_empty_snapshots_yield_empty_rates() {
    let r = io_rates(&[], &[], Duration::from_secs(1)).unwrap();
    assert!(r.is_empty());
}

#[test]
fn io_device_added_between_snapshots_is_skipped() {
    // 8:0 only appears in `b`; we have no baseline counter to subtract.
    let a = vec![iodev(259, 0, 0, 0)];
    let b = vec![iodev(8, 0, 1000, 0), iodev(259, 0, 5000, 0)];
    let r = io_rates(&a, &b, Duration::from_secs(1)).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].major, r[0].minor), (259, 0));
}

#[test]
fn io_device_removed_between_snapshots_is_omitted() {
    // 8:0 only appears in `a`; the device went away.
    let a = vec![iodev(8, 0, 0, 0), iodev(259, 0, 0, 0)];
    let b = vec![iodev(259, 0, 1000, 0)];
    let r = io_rates(&a, &b, Duration::from_secs(1)).unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].major, r[0].minor), (259, 0));
}

#[test]
fn io_counter_regression_on_one_device_flags_reset_for_that_device_only() {
    let a = vec![iodev(8, 0, 1000, 1000), iodev(259, 0, 0, 0)];
    let b = vec![iodev(8, 0, 500, 1000), iodev(259, 0, 5000, 0)];
    let r = io_rates(&a, &b, Duration::from_secs(1)).unwrap();
    let dev_8 = r.iter().find(|x| x.major == 8).unwrap();
    let dev_259 = r.iter().find(|x| x.major == 259).unwrap();
    assert!(dev_8.reset_detected);
    approx(dev_8.rbytes_per_sec, 0.0);
    assert!(!dev_259.reset_detected);
    approx(dev_259.rbytes_per_sec, 5000.0);
}

#[test]
fn io_rates_are_sorted_by_major_minor_for_determinism() {
    // Provide unsorted input; expect sorted output.
    let a = vec![iodev(259, 1, 0, 0), iodev(8, 0, 0, 0), iodev(259, 0, 0, 0)];
    let b = vec![
        iodev(259, 1, 100, 0),
        iodev(8, 0, 200, 0),
        iodev(259, 0, 300, 0),
    ];
    let r = io_rates(&a, &b, Duration::from_secs(1)).unwrap();
    let keys: Vec<_> = r.iter().map(|x| (x.major, x.minor)).collect();
    assert_eq!(keys, vec![(8, 0), (259, 0), (259, 1)]);
}

#[test]
fn io_full_field_coverage() {
    // Verify all six counter pairs (rbytes/wbytes/rios/wios/dbytes/dios)
    // map to their corresponding _per_sec field.
    let a = vec![IoDeviceStat {
        major: 1,
        minor: 1,
        rbytes: 0,
        wbytes: 0,
        rios: 0,
        wios: 0,
        dbytes: 0,
        dios: 0,
    }];
    let b = vec![IoDeviceStat {
        major: 1,
        minor: 1,
        rbytes: 100,
        wbytes: 200,
        rios: 10,
        wios: 20,
        dbytes: 300,
        dios: 30,
    }];
    let r = io_rates(&a, &b, Duration::from_secs(2)).unwrap();
    approx(r[0].rbytes_per_sec, 50.0);
    approx(r[0].wbytes_per_sec, 100.0);
    approx(r[0].rios_per_sec, 5.0);
    approx(r[0].wios_per_sec, 10.0);
    approx(r[0].dbytes_per_sec, 150.0);
    approx(r[0].dios_per_sec, 15.0);
}
