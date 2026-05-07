use cgroup_mcp::collector::pressure::{parse_pressure, read_pressure, Pressure, PressureLine};
use std::path::Path;

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pressure")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading fixture {}: {e}", path.display()))
}

#[test]
fn parses_memory_pressure_with_some_and_full() {
    let p = parse_pressure(&fixture("memory_normal")).unwrap();
    assert_eq!(
        p,
        Pressure {
            some: PressureLine {
                avg10: 1.23,
                avg60: 2.34,
                avg300: 3.45,
                total_usec: 12345678,
            },
            full: Some(PressureLine {
                avg10: 0.50,
                avg60: 1.00,
                avg300: 1.50,
                total_usec: 987654,
            }),
        }
    );
}

#[test]
fn parses_cpu_pressure_with_only_some() {
    // /proc/pressure/cpu has no `full` line — CPU is never fully stalled
    // for non-idle tasks the way memory or IO can be.
    let p = parse_pressure(&fixture("cpu_some_only")).unwrap();
    assert!(p.full.is_none());
    assert_eq!(p.some.avg10, 0.42);
    assert_eq!(p.some.total_usec, 54321);
}

#[test]
fn parses_zero_pressure() {
    let p = parse_pressure(&fixture("memory_zero")).unwrap();
    assert_eq!(p.some.avg10, 0.0);
    assert_eq!(p.some.total_usec, 0);
    assert_eq!(p.full.unwrap().total_usec, 0);
}

#[test]
fn read_pressure_from_path_works() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pressure/memory_normal");
    let p = read_pressure(&path).unwrap();
    assert!(p.full.is_some());
}

#[test]
fn rejects_missing_required_field() {
    let bad = "some avg10=1.0 avg60=2.0 total=100\n"; // no avg300
    let err = parse_pressure(bad).unwrap_err();
    assert!(err.to_string().contains("avg300"), "error was: {err}");
}

#[test]
fn rejects_unknown_line_kind() {
    let bad = "partial avg10=0 avg60=0 avg300=0 total=0\n";
    let err = parse_pressure(bad).unwrap_err();
    assert!(err.to_string().contains("partial"), "error was: {err}");
}

#[test]
fn ignores_unknown_fields_for_forward_compat() {
    // Kernel may add new fields; we shouldn't break.
    let s = "some avg10=1.0 avg60=2.0 avg300=3.0 total=100 future_field=42\n";
    let p = parse_pressure(s).unwrap();
    assert_eq!(p.some.avg10, 1.0);
}

#[test]
fn rejects_missing_some_line() {
    // `full` alone is meaningless — `some` is required.
    let only_full = "full avg10=1.0 avg60=2.0 avg300=3.0 total=100\n";
    let err = parse_pressure(only_full).unwrap_err();
    assert!(err.to_string().contains("some"), "error was: {err}");
}
