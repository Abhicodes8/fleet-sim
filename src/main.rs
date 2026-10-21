//! stdin → stdout driver. One response line per non-blank input line,
//! flushed immediately so an interactive harness never waits on a buffer.

use std::io::{self, BufRead, BufWriter, Write};

use fleet_sim::protocol::error_response;
use fleet_sim::{SimError, Simulator};

const USAGE: &str = "\
fleet-sim: discrete-time fleet simulation engine

Reads newline-delimited JSON commands on stdin and writes exactly one JSON
response line per command on stdout.

Usage:
  fleet-sim < commands.jsonl
  cargo run --release < examples/cargo.jsonl

Commands: init, tick, set_destination, dock, undock, load, unload, swap,
          get_state, get_state_at   (see README.md)
";

fn main() {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "-h" | "--help" => print!("{USAGE}"),
            "-V" | "--version" => println!("fleet-sim {}", env!("CARGO_PKG_VERSION")),
            other => {
                eprintln!("fleet-sim: unexpected argument '{other}'\n\n{USAGE}");
                std::process::exit(2);
            }
        }
        return;
    }

    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let mut sim = Simulator::new();
    let mut buf = Vec::new();

    loop {
        buf.clear();
        match input.read_until(b'\n', &mut buf) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => {
                eprintln!("fleet-sim: failed to read stdin: {e}");
                std::process::exit(1);
            }
        }

        // Invalid UTF-8 is a bad command, not a crash.
        let response = match std::str::from_utf8(&buf) {
            Ok(line) => {
                // Tolerate a UTF-8 byte-order mark, which some editors prepend.
                let line = line.trim_start_matches('\u{feff}').trim();
                if line.is_empty() {
                    continue; // blank lines are not commands
                }
                sim.process(line)
            }
            Err(_) => error_response(&SimError::InvalidUtf8),
        };

        // A closed stdout (e.g. piped into `head`) ends the run quietly.
        if writeln!(out, "{response}")
            .and_then(|()| out.flush())
            .is_err()
        {
            break;
        }
    }
}
