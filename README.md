# fleet-sim

A discrete-time simulation engine for a fleet of vessels, driven by
newline-delimited JSON over stdin/stdout. Implements the base problem,
Follow-on 1 (cargo and docks), and Follow-on 2 (historical queries).

Rust 2024 edition, one dependency (`serde_json`).

**Design doc:** [`DESIGN_DOC.pdf`](DESIGN_DOC.pdf)

**Requirements:** Rust 1.85 or newer (for the 2024 edition). Install from
https://rustup.rs if needed. `make` is optional; every target has a plain
`cargo` equivalent below.

## Quick start

```bash
make build                                         # target/release/fleet-sim
./target/release/fleet-sim < examples/cargo.jsonl  # replay a session
make run                                           # interactive: type commands
```

Without `make`:

```bash
cargo build --release
cargo run --release < examples/base.jsonl
```

## Testing

```bash
make test       # full suite (cargo test --release)
make examples   # diff each examples/*.jsonl against its golden output
make lint       # rustfmt check + clippy with warnings as errors
```

| Suite | What it proves |
|---|---|
| `tests/spec_examples.rs` | All three worked examples from the PDF match. The PDF truncates long lines, so each visible line is checked as a prefix, and the full output is pinned by golden files. |
| `tests/errors.rs` | Every `SimError` variant has a test asserting its exact message, and failed commands are shown to leave state untouched (invalid UTF-8 is exercised end-to-end in `tests/cli.rs`). A compile-time exhaustive match fails if a variant is added without a test. |
| `tests/semantics.rs` | Tick semantics, committed vs. planned state, every "left to the implementer" decision, throughput accounting, delivery, atomicity, history. |
| `tests/invariants.rs` | 600 random sessions, about 220k commands of mixed valid and invalid input. After every command it checks: errors never mutate state; cargo is conserved; history snapshots never change; no dock exceeds its throughput; in-transit vessels always arrive in the future. Seeded, so runs are reproducible and deterministic. |
| `tests/cli.rs` | The real binary over pipes: golden files, garbage input, invalid UTF-8, blank lines, CRLF, missing trailing newline. |

## Protocol summary

Each input line is one command. Each command produces exactly one output line:

```
{"status":"ok","payload":<WorldState>}
{"status":"error","payload":{"message":"<reason>"}}
```

| Command | Parameters | Allowed when (committed state) | Returns |
|---|---|---|---|
| `init` | `travel_times`, `vessels`, `dock_throughput`?, `cargo_manifest`? | first command only | state at tick 0 |
| `tick` | `cargo_manifest`? | always | new committed state |
| `set_destination` | `vessel_id`, `destination` | vessel idle | planned state |
| `dock` | `vessel_id` | vessel idle | planned state |
| `undock` | `vessel_id` | vessel docked | planned state |
| `load` | `vessel_id`, `destination` | docked, dock has that cargo | planned state |
| `unload` | `vessel_id`, `destination` | docked, vessel has that cargo | planned state |
| `swap` | `vessel_id`, `load_destination`, `unload_destination` | docked, both units exist | planned state |
| `get_state` | none | always | planned state |
| `get_state_at` | `tick` | 0 ≤ tick ≤ current | committed state at that tick |

### Base mode vs. cargo mode

`dock_throughput` on `init` switches on Follow-on 1. Without it, the engine runs
the base problem exactly as specified: responses have no `cargo` or `docks`
fields, and cargo commands return an error. This lets one binary reproduce
every example in the PDF byte for byte.

### World state shape (cargo mode)

```json
{"tick":2,
 "vessels":{"v1":{"state":"docked","location":0,"cargo":[{"destination":2,"quantity":1}]}},
 "docks":{"0":{"cargo":[{"destination":1,"quantity":1}]},"1":{"cargo":[]},"2":{"cargo":[]}}}
```

Vessels appear in `init` order. Cargo entries appear in the order a
destination was first added, which matches the PDF examples. The PDF
truncates the `docks` field, so its shape here is an assumption: one entry
per location, keyed by index, mirroring how vessels carry `cargo`.

## Behavior reference

All validity checks read the **committed** state; effects land in the
**planned** state. Movement commands for a vessel replace each other, while
cargo operations accumulate. A rejected command never changes anything.

| Situation | Behavior |
|---|---|
| `set_destination` to the current location | error: `vessel v1 is already at location 0` |
| `set_destination` ↔ `dock` before a tick | latest wins |
| `dock` after `dock` | ok, no change |
| `undock` after a pending `dock` | error: not docked (committed is idle) |
| `undock` after `undock` | ok, no change |
| `dock` after a pending `undock` | error: not idle (committed is docked) |
| `undock` after load/unload/swap, or the reverse | allowed: cargo work happens at the dock, then the vessel leaves on tick |
| load then unload of the same cargo | cancels out, costs 0 operations |
| throughput cost | fewest operations that reach the planned cargo: per vessel, max(net loads, net unloads), summed over vessels at the dock |
| `swap` with the same destination on both sides | error |
| manifest cargo whose destination is its own location | error (it would already be delivered) |
| a bad manifest entry | whole command rejected, including the tick |
| blank input lines | ignored (no output) |
| missing or `null` `parameters` | treated as `{}` |

## Layout

```
src/
  error.rs     every failure mode as one enum; its Display is the message text
  protocol.rs  line → typed Command, with field-precise parameter errors
  model.rs     World / Vessel / CargoHold as plain data, and JSON output
  engine.rs    committed + pending + planned state, tick, history
  main.rs      stdin/stdout loop
tests/         see the Testing table above
examples/      sessions (*.jsonl) with golden outputs (*.expected.jsonl)
scripts/       check_examples.sh
```

## Performance

Every response contains the full world, so output size dominates run time.
A 40,000-command session with 50 vessels and 20 docks (about 1.6 GB of
output) runs in roughly 7 seconds in release mode. History stores one
snapshot per tick: O(ticks × world size) memory, with O(1) lookup.
