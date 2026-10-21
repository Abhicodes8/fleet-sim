//! The three worked examples from the project PDF.
//!
//! Each example is replayed from `examples/*.jsonl` (with the ticks the PDF
//! omits filled in). Two checks per example:
//!
//! 1. Every response the PDF shows must match. The PDF truncates long lines,
//!    so its visible text is checked as a prefix of our output.
//! 2. The full output must equal `examples/*.expected.jsonl` byte for byte,
//!    which pins everything the PDF cut off.

mod common;

use common::Session;

fn run(fixture: &str) -> Vec<String> {
    let mut s = Session::new();
    fixture
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| s.sim.process(l))
        .collect()
}

fn check(name: &str, input: &str, expected: &str, pdf: &[(usize, &str)]) {
    let out = run(input);
    for (i, prefix) in pdf {
        assert!(
            out[*i].starts_with(prefix),
            "{name} response #{i} does not match the PDF.\n  pdf: {prefix}\n  got: {}",
            out[*i]
        );
    }
    let expected: Vec<&str> = expected.lines().collect();
    assert_eq!(
        out.len(),
        expected.len(),
        "{name}: wrong number of responses"
    );
    for (i, (got, want)) in out.iter().zip(&expected).enumerate() {
        assert_eq!(got, want, "{name} response #{i} differs from golden file");
    }
}

#[test]
fn base_problem_example() {
    check(
        "base",
        include_str!("../examples/base.jsonl"),
        include_str!("../examples/base.expected.jsonl"),
        &[
            (
                0,
                r#"{"status":"ok","payload":{"tick":0,"vessels":{"v1":{"state":"idle","location":0},"v2":{"state":"idle","location"#,
            ),
            (
                1,
                r#"{"status":"ok","payload":{"tick":0,"vessels":{"v1":{"state":"transit","from":0,"to":2,"arrives_at_tick":7},"v2""#,
            ),
            (
                2,
                r#"{"status":"ok","payload":{"tick":0,"vessels":{"v1":{"state":"transit","from":0,"to":1,"arrives_at_tick":4},"v2""#,
            ),
            (
                3,
                r#"{"status":"ok","payload":{"tick":1,"vessels":{"v1":{"state":"transit","from":0,"to":1,"arrives_at_tick":4},"v2""#,
            ),
            (
                4,
                r#"{"status":"error","payload":{"message":"vessel v1 is not idle"}}"#,
            ),
            (
                7,
                r#"{"status":"ok","payload":{"tick":4,"vessels":{"v1":{"state":"idle","location":1},"v2":{"state":"idle","location"#,
            ),
            (
                8,
                r#"{"status":"ok","payload":{"tick":4,"vessels":{"v1":{"state":"transit","from":1,"to":2,"arrives_at_tick":7},"v2""#,
            ),
        ],
    );
}

#[test]
fn cargo_and_docks_example() {
    check(
        "cargo",
        include_str!("../examples/cargo.jsonl"),
        include_str!("../examples/cargo.expected.jsonl"),
        &[
            (
                0,
                r#"{"status":"ok","payload":{"tick":0,"vessels":{"v1":{"state":"idle","location":0,"cargo":[]},"v2":{"state":"idle"#,
            ),
            (
                1,
                r#"{"status":"ok","payload":{"tick":1,"vessels":{"v1":{"state":"idle","location":0,"cargo":[]},"v2":{"state":"idle"#,
            ),
            (
                2,
                r#"{"status":"ok","payload":{"tick":1,"vessels":{"v1":{"state":"docked","location":0,"cargo":[]},"v2":{"state":"id"#,
            ),
            (
                3,
                r#"{"status":"ok","payload":{"tick":2,"vessels":{"v1":{"state":"docked","location":0,"cargo":[]},"v2":{"state":"id"#,
            ),
            (
                4,
                r#"{"status":"ok","payload":{"tick":2,"vessels":{"v1":{"state":"docked","location":0,"cargo":[{"destination":2,"qu"#,
            ),
            (
                5,
                r#"{"status":"ok","payload":{"tick":2,"vessels":{"v1":{"state":"docked","location":0,"cargo":[{"destination":2,"qu"#,
            ),
            (
                6,
                r#"{"status":"ok","payload":{"tick":3,"vessels":{"v1":{"state":"docked","location":0,"cargo":[{"destination":2,"qu"#,
            ),
            (
                7,
                r#"{"status":"ok","payload":{"tick":3,"vessels":{"v1":{"state":"idle","location":0,"cargo":[{"destination":2,"quan"#,
            ),
            (
                8,
                r#"{"status":"ok","payload":{"tick":4,"vessels":{"v1":{"state":"idle","location":0,"cargo":[{"destination":2,"quan"#,
            ),
            (
                9,
                r#"{"status":"ok","payload":{"tick":4,"vessels":{"v1":{"state":"transit","from":0,"to":2,"arrives_at_tick":11,"car"#,
            ),
            (
                16,
                r#"{"status":"ok","payload":{"tick":11,"vessels":{"v1":{"state":"idle","location":2,"cargo":[{"destination":2,"qua"#,
            ),
            (
                17,
                r#"{"status":"ok","payload":{"tick":11,"vessels":{"v1":{"state":"docked","location":2,"cargo":[{"destination":2,"q"#,
            ),
            (
                18,
                r#"{"status":"ok","payload":{"tick":12,"vessels":{"v1":{"state":"docked","location":2,"cargo":[{"destination":2,"q"#,
            ),
            (
                19,
                r#"{"status":"ok","payload":{"tick":12,"vessels":{"v1":{"state":"docked","location":2,"cargo":[{"destination":1,"q"#,
            ),
            (
                20,
                r#"{"status":"ok","payload":{"tick":13,"vessels":{"v1":{"state":"docked","location":2,"cargo":[{"destination":1,"q"#,
            ),
        ],
    );
}

#[test]
fn history_example() {
    check(
        "history",
        include_str!("../examples/history.jsonl"),
        include_str!("../examples/history.expected.jsonl"),
        &[
            (
                21,
                r#"{"status":"ok","payload":{"tick":1,"vessels":{"v1":{"state":"idle","location":0,"cargo":[]},"v2":{"state":"idle"#,
            ),
            (
                22,
                r#"{"status":"ok","payload":{"tick":5,"vessels":{"v1":{"state":"transit","from":0,"to":2,"arrives_at_tick"#,
            ),
            (
                23,
                r#"{"status":"ok","payload":{"tick":13,"vessels":{"v1":{"state":"docked","location":2,"cargo":[{"destination":1,"q"#,
            ),
            (
                24,
                r#"{"status":"error","payload":{"message":"tick 99 is in the future (current tick: 13)"}}"#,
            ),
            (
                25,
                r#"{"status":"ok","payload":{"tick":13,"vessels":{"v1":{"state":"docked","location":2,"cargo":[{"destination":1,"q"#,
            ),
        ],
    );
}

/// The PDF's standalone swap snippet: at tick 2, v1 is docked at 0 holding
/// cargo for 2, and dock 0 holds cargo for 1.
#[test]
fn swap_snippet() {
    let mut s = Session::new();
    s.ok(r#"{"command":"init","parameters":{"travel_times":[[0,4,7],[4,0,3],[7,3,0]],"dock_throughput":[2,4,1],"vessels":[{"id":"v1","location":0},{"id":"v2","location":2}],"cargo_manifest":[{"location":0,"destination":2,"quantity":1},{"location":0,"destination":1,"quantity":1}]}}"#);
    s.ok(&common::dock("v1"));
    s.tick();
    s.ok(&common::load("v1", 2));
    s.tick();
    let line = s.sim.process(&common::swap("v1", 1, 2));
    assert!(line.starts_with(
        r#"{"status":"ok","payload":{"tick":2,"vessels":{"v1":{"state":"docked","location":0,"cargo":[{"destinatio"#
    ), "got {line}");
    let p: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(
        common::cargo(&p["payload"]["vessels"]["v1"]["cargo"]),
        vec![(1, 1)]
    );
    assert_eq!(
        common::cargo(&p["payload"]["docks"]["0"]["cargo"]),
        vec![(2, 1)]
    );
}
