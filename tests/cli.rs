//! End-to-end: run the real binary over stdin/stdout.

use std::io::Write;
use std::process::{Command, Stdio};

fn run(input: &[u8]) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fleet-sim"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary starts");
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "binary exited with {}", out.status);
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn examples_match_golden_files() {
    for (input, expected) in [
        (
            include_str!("../examples/base.jsonl"),
            include_str!("../examples/base.expected.jsonl"),
        ),
        (
            include_str!("../examples/cargo.jsonl"),
            include_str!("../examples/cargo.expected.jsonl"),
        ),
        (
            include_str!("../examples/history.jsonl"),
            include_str!("../examples/history.expected.jsonl"),
        ),
        (
            include_str!("../examples/edge_cases.jsonl"),
            include_str!("../examples/edge_cases.expected.jsonl"),
        ),
    ] {
        assert_eq!(run(input.as_bytes()), expected);
    }
}

#[test]
fn one_response_per_command_even_for_garbage() {
    let input = b"{bad json\n\xff\xfe\n[]\n{\"command\":\"tick\"}\n";
    let out = run(input);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 4);
    assert!(lines[0].contains("invalid JSON"));
    assert!(lines[1].contains("not valid UTF-8"));
    assert!(lines[2].contains("must be a JSON object"));
    assert!(lines[3].contains("not initialized"));
}

#[test]
fn blank_lines_and_crlf_are_tolerated() {
    let input = b"\n   \n{\"command\":\"init\",\"parameters\":{\"travel_times\":[[0]],\"vessels\":[]}}\r\n\n{\"command\":\"tick\"}\r\n";
    assert_eq!(
        run(input),
        "{\"status\":\"ok\",\"payload\":{\"tick\":0,\"vessels\":{}}}\n{\"status\":\"ok\",\"payload\":{\"tick\":1,\"vessels\":{}}}\n"
    );
}

#[test]
fn missing_trailing_newline_still_processed() {
    let out = run(br#"{"command":"get_state"}"#);
    assert_eq!(out.lines().count(), 1);
}

#[test]
fn empty_input_produces_no_output() {
    assert_eq!(run(b""), "");
}

#[test]
fn leading_byte_order_mark_is_ignored() {
    let out = run(b"\xef\xbb\xbf{\"command\":\"init\",\"parameters\":{\"travel_times\":[[0]],\"vessels\":[]}}\n");
    assert!(out.starts_with(r#"{"status":"ok""#), "got {out}");
}
