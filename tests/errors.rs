//! One test per `SimError` variant: the command that triggers it, the exact
//! message, and (where a world exists) proof that nothing changed.
//!
//! `every_variant_is_covered` at the bottom fails to compile if a variant is
//! added to the enum without being listed here.

mod common;

use common::*;
use fleet_sim::SimError;

const INIT_BASE: &str = r#"{"command":"init","parameters":{"travel_times":[[0,4,7],[4,0,3],[7,3,0]],"vessels":[{"id":"v1","location":0}]}}"#;

fn init_with(params: &str) -> String {
    format!(r#"{{"command":"init","parameters":{params}}}"#)
}

// ---- Protocol ----------------------------------------------------------------

#[test]
fn malformed_json() {
    let mut s = Session::new();
    assert!(s.err("{not json").starts_with("invalid JSON: "));
    assert!(s.err(r#"{"command":"tick""#).starts_with("invalid JSON: "));
}

#[test]
fn not_an_object() {
    let mut s = Session::new();
    assert_eq!(s.err("[1,2,3]"), "command must be a JSON object");
    assert_eq!(s.err("42"), "command must be a JSON object");
}

#[test]
fn missing_command() {
    let mut s = Session::new();
    assert_eq!(
        s.err(r#"{"parameters":{}}"#),
        "missing or non-string 'command' field"
    );
    assert_eq!(
        s.err(r#"{"command":7}"#),
        "missing or non-string 'command' field"
    );
}

#[test]
fn unknown_command() {
    let mut s = Session::base();
    assert_eq!(
        s.err_unchanged(r#"{"command":"teleport"}"#),
        "unknown command 'teleport'"
    );
}

#[test]
fn parameters_must_be_object() {
    let mut s = Session::base();
    assert_eq!(
        s.err_unchanged(r#"{"command":"tick","parameters":[]}"#),
        "parameter 'parameters' must be an object"
    );
}

// ---- Lifecycle ---------------------------------------------------------------

#[test]
fn not_initialized() {
    let mut s = Session::new();
    let msg = "simulation is not initialized: the first command must be init";
    assert_eq!(s.err(r#"{"command":"tick","parameters":{}}"#), msg);
    assert_eq!(s.err(r#"{"command":"get_state","parameters":{}}"#), msg);
    assert_eq!(s.err(&set_dest("v1", 1)), msg);
    // Lifecycle is checked before parameter shape.
    assert_eq!(
        s.err(r#"{"command":"tick","parameters":{"cargo_manifest":5}}"#),
        msg
    );
}

#[test]
fn already_initialized() {
    let mut s = Session::base();
    assert_eq!(
        s.err_unchanged(INIT_BASE),
        "simulation is already initialized"
    );
}

#[test]
fn failed_init_can_be_retried() {
    let mut s = Session::new();
    s.err(&init_with(r#"{"travel_times":[],"vessels":[]}"#));
    s.ok(INIT_BASE);
}

// ---- Parameter shape ---------------------------------------------------------

#[test]
fn missing_param() {
    let mut s = Session::base();
    assert_eq!(
        s.err_unchanged(r#"{"command":"set_destination","parameters":{"vessel_id":"v1"}}"#),
        "missing parameter 'destination'"
    );
    assert_eq!(
        s.err_unchanged(r#"{"command":"get_state_at","parameters":{}}"#),
        "missing parameter 'tick'"
    );
    let mut fresh = Session::new();
    assert_eq!(
        fresh.err(&init_with(r#"{"vessels":[]}"#)),
        "missing parameter 'travel_times'"
    );
    assert_eq!(
        fresh.err(&init_with(
            r#"{"travel_times":[[0]],"vessels":[{"id":"a"}]}"#
        )),
        "missing parameter 'vessels[0].location'"
    );
}

#[test]
fn invalid_param() {
    let mut s = Session::base();
    assert_eq!(
        s.err_unchanged(
            r#"{"command":"set_destination","parameters":{"vessel_id":"v1","destination":1.5}}"#
        ),
        "parameter 'destination' must be an integer"
    );
    assert_eq!(
        s.err_unchanged(
            r#"{"command":"set_destination","parameters":{"vessel_id":"v1","destination":"1"}}"#
        ),
        "parameter 'destination' must be an integer"
    );
    assert_eq!(
        s.err_unchanged(
            r#"{"command":"set_destination","parameters":{"vessel_id":1,"destination":1}}"#
        ),
        "parameter 'vessel_id' must be a string"
    );
    let mut c = Session::cargo();
    assert_eq!(
        c.err_unchanged(r#"{"command":"tick","parameters":{"cargo_manifest":[{"location":0,"destination":1,"quantity":"2"}]}}"#),
        "parameter 'cargo_manifest[0].quantity' must be an integer"
    );
    let mut fresh = Session::new();
    assert_eq!(
        fresh.err(&init_with(
            r#"{"travel_times":[[0,1],[1,"x"]],"vessels":[]}"#
        )),
        "parameter 'travel_times[1][1]' must be an integer"
    );
}

// ---- World definition --------------------------------------------------------

#[test]
fn empty_world() {
    let mut s = Session::new();
    assert_eq!(
        s.err(&init_with(r#"{"travel_times":[],"vessels":[]}"#)),
        "travel_times must contain at least one location"
    );
}

#[test]
fn matrix_not_square() {
    let mut s = Session::new();
    assert_eq!(
        s.err(&init_with(r#"{"travel_times":[[0,1],[1]],"vessels":[]}"#)),
        "travel_times row 1 has 1 entries, expected 2"
    );
}

#[test]
fn non_zero_diagonal() {
    let mut s = Session::new();
    assert_eq!(
        s.err(&init_with(r#"{"travel_times":[[0,1],[1,3]],"vessels":[]}"#)),
        "travel_times[1][1] must be 0 (got 3)"
    );
}

#[test]
fn non_positive_travel_time() {
    let mut s = Session::new();
    assert_eq!(
        s.err(&init_with(r#"{"travel_times":[[0,0],[1,0]],"vessels":[]}"#)),
        "travel_times[0][1] must be a positive integer (got 0)"
    );
    assert_eq!(
        s.err(&init_with(
            r#"{"travel_times":[[0,2],[-1,0]],"vessels":[]}"#
        )),
        "travel_times[1][0] must be a positive integer (got -1)"
    );
}

#[test]
fn throughput_length() {
    let mut s = Session::new();
    assert_eq!(
        s.err(&init_with(
            r#"{"travel_times":[[0,1],[1,0]],"dock_throughput":[1],"vessels":[]}"#
        )),
        "dock_throughput has 1 entries, expected 2 (one per location)"
    );
}

#[test]
fn non_positive_throughput() {
    let mut s = Session::new();
    assert_eq!(
        s.err(&init_with(
            r#"{"travel_times":[[0,1],[1,0]],"dock_throughput":[1,0],"vessels":[]}"#
        )),
        "dock_throughput[1] must be a positive integer (got 0)"
    );
}

#[test]
fn empty_vessel_id() {
    let mut s = Session::new();
    assert_eq!(
        s.err(&init_with(
            r#"{"travel_times":[[0]],"vessels":[{"id":"","location":0}]}"#
        )),
        "vessels[0].id must be a non-empty string"
    );
}

#[test]
fn duplicate_vessel() {
    let mut s = Session::new();
    assert_eq!(
        s.err(&init_with(
            r#"{"travel_times":[[0]],"vessels":[{"id":"a","location":0},{"id":"a","location":0}]}"#
        )),
        "duplicate vessel id a"
    );
}

// ---- References into the world -----------------------------------------------

#[test]
fn unknown_vessel() {
    let mut s = Session::base();
    assert_eq!(s.err_unchanged(&set_dest("v9", 1)), "unknown vessel v9");
    let mut c = Session::cargo();
    assert_eq!(c.err_unchanged(&dock("ghost")), "unknown vessel ghost");
    assert_eq!(c.err_unchanged(&load("ghost", 1)), "unknown vessel ghost");
}

#[test]
fn location_out_of_range() {
    let mut s = Session::base();
    assert_eq!(
        s.err_unchanged(&set_dest("v1", 3)),
        "destination 3 is out of range (valid locations are 0 to 2)"
    );
    assert_eq!(
        s.err_unchanged(&set_dest("v1", -1)),
        "destination -1 is out of range (valid locations are 0 to 2)"
    );
    let mut c = Session::cargo();
    assert_eq!(
        c.err_unchanged(r#"{"command":"tick","parameters":{"cargo_manifest":[{"location":0,"destination":1,"quantity":1},{"location":5,"destination":1,"quantity":1}]}}"#),
        "cargo_manifest[1].location 5 is out of range (valid locations are 0 to 2)"
    );
    assert_eq!(
        c.err_unchanged(&swap("v1", 1, 9)),
        "unload_destination 9 is out of range (valid locations are 0 to 2)"
    );
    let mut fresh = Session::new();
    assert_eq!(
        fresh.err(&init_with(
            r#"{"travel_times":[[0]],"vessels":[{"id":"a","location":1}]}"#
        )),
        "vessels[0].location 1 is out of range (valid locations are 0 to 0)"
    );
}

#[test]
fn non_positive_quantity() {
    let mut c = Session::cargo();
    assert_eq!(
        c.err_unchanged(r#"{"command":"tick","parameters":{"cargo_manifest":[{"location":0,"destination":1,"quantity":0}]}}"#),
        "cargo_manifest[0].quantity must be a positive integer (got 0)"
    );
}

#[test]
fn manifest_already_at_destination() {
    let mut c = Session::cargo();
    assert_eq!(
        c.err_unchanged(r#"{"command":"tick","parameters":{"cargo_manifest":[{"location":2,"destination":2,"quantity":1}]}}"#),
        "cargo_manifest[0]: cargo bound for location 2 cannot originate there"
    );
}

// ---- Vessel state machine ----------------------------------------------------

#[test]
fn not_idle() {
    let mut s = Session::base();
    s.ok(&set_dest("v1", 1));
    s.tick();
    assert_eq!(s.err_unchanged(&set_dest("v1", 2)), "vessel v1 is not idle");
    let mut c = Session::cargo();
    c.ok(&dock("v1"));
    c.tick();
    assert_eq!(c.err_unchanged(&set_dest("v1", 1)), "vessel v1 is not idle");
    assert_eq!(c.err_unchanged(&dock("v1")), "vessel v1 is not idle");
}

#[test]
fn not_docked() {
    let mut c = Session::cargo();
    assert_eq!(c.err_unchanged(&undock("v1")), "vessel v1 is not docked");
    assert_eq!(c.err_unchanged(&load("v1", 1)), "vessel v1 is not docked");
    assert_eq!(c.err_unchanged(&unload("v1", 1)), "vessel v1 is not docked");
    assert_eq!(
        c.err_unchanged(&swap("v1", 1, 2)),
        "vessel v1 is not docked"
    );
}

#[test]
fn already_at_location() {
    let mut s = Session::base();
    assert_eq!(
        s.err_unchanged(&set_dest("v1", 0)),
        "vessel v1 is already at location 0"
    );
}

// ---- Cargo -------------------------------------------------------------------

#[test]
fn cargo_disabled() {
    let mut s = Session::base();
    let msg = "cargo operations are disabled: init did not provide dock_throughput";
    assert_eq!(s.err_unchanged(&dock("v1")), msg);
    assert_eq!(s.err_unchanged(&undock("v1")), msg);
    assert_eq!(s.err_unchanged(&load("v1", 1)), msg);
    assert_eq!(s.err_unchanged(&unload("v1", 1)), msg);
    assert_eq!(s.err_unchanged(&swap("v1", 1, 2)), msg);
    assert_eq!(
        s.err_unchanged(r#"{"command":"tick","parameters":{"cargo_manifest":[]}}"#),
        msg
    );
    let mut fresh = Session::new();
    assert_eq!(
        fresh.err(&init_with(
            r#"{"travel_times":[[0,1],[1,0]],"vessels":[],"cargo_manifest":[{"location":0,"destination":1,"quantity":1}]}"#
        )),
        msg
    );
}

#[test]
fn no_dock_inventory() {
    let mut c = Session::cargo();
    c.ok(&dock("v1"));
    c.tick();
    assert_eq!(
        c.err_unchanged(&load("v1", 2)),
        "dock at location 0 has no cargo for destination 2"
    );
}

#[test]
fn no_vessel_cargo() {
    let mut c = Session::cargo();
    c.ok(&dock("v1"));
    c.tick();
    assert_eq!(
        c.err_unchanged(&unload("v1", 2)),
        "vessel v1 has no cargo for destination 2"
    );
}

#[test]
fn swap_same_cargo() {
    let mut c = Session::cargo();
    c.ok(&dock("v1"));
    c.tick();
    assert_eq!(
        c.err_unchanged(&swap("v1", 1, 1)),
        "swap for vessel v1 must exchange different cargo (both sides are destination 1)"
    );
}

#[test]
fn throughput_exceeded() {
    let mut c = Session::cargo();
    c.manifest_tick(r#"[{"location":0,"destination":2,"quantity":5}]"#);
    c.ok(&dock("v1"));
    c.tick();
    c.ok(&load("v1", 2));
    c.ok(&load("v1", 2));
    assert_eq!(
        c.err_unchanged(&load("v1", 2)),
        "transfer of 3 operations exceeds dock throughput of 2 at location 0"
    );
}

// ---- History -----------------------------------------------------------------

#[test]
fn negative_tick() {
    let mut s = Session::base();
    assert_eq!(s.err_unchanged(&state_at(-1)), "tick -1 is negative");
}

#[test]
fn future_tick() {
    let mut s = Session::base();
    s.ticks(3);
    assert_eq!(
        s.err_unchanged(&state_at(4)),
        "tick 4 is in the future (current tick: 3)"
    );
}

// ---- Numeric limits ----------------------------------------------------------

#[test]
fn overflow() {
    // A trip so long its arrival tick cannot be represented.
    let big = i64::MAX;
    let mut s = Session::new();
    s.ok(&init_with(&format!(
        r#"{{"travel_times":[[0,{big}],[{big},0]],"vessels":[{{"id":"a","location":0}}]}}"#
    )));
    s.tick();
    // tick (1) + i64::MAX still fits in u64, so this one is accepted...
    s.ok(&set_dest("a", 1));
    // ...but cargo quantities past u64::MAX are rejected, atomically.
    let mut c = Session::cargo();
    let entry = format!(r#"{{"location":0,"destination":1,"quantity":{big}}}"#);
    c.manifest_tick(&format!("[{entry},{entry}]"));
    let msg = c.err_unchanged(&format!(
        r#"{{"command":"tick","parameters":{{"cargo_manifest":[{entry},{entry},{entry}]}}}}"#
    ));
    assert_eq!(msg, "cargo quantity overflowed");
}

#[test]
fn invalid_utf8_message() {
    assert_eq!(
        SimError::InvalidUtf8.to_string(),
        "input line is not valid UTF-8"
    );
}

/// Compile-time guard: adding a variant without a test above breaks this match.
#[test]
fn every_variant_is_covered() {
    fn _exhaustive(e: SimError) {
        use SimError::*;
        match e {
            InvalidUtf8
            | MalformedJson(_)
            | NotAnObject
            | MissingCommand
            | UnknownCommand(_)
            | NotInitialized
            | AlreadyInitialized
            | MissingParam(_)
            | InvalidParam { .. }
            | EmptyWorld
            | MatrixNotSquare { .. }
            | NonZeroDiagonal { .. }
            | NonPositiveTravelTime { .. }
            | ThroughputLength { .. }
            | NonPositiveThroughput { .. }
            | EmptyVesselId { .. }
            | DuplicateVessel(_)
            | UnknownVessel(_)
            | LocationOutOfRange { .. }
            | NonPositiveQuantity { .. }
            | ManifestAlreadyAtDestination { .. }
            | NotIdle(_)
            | NotDocked(_)
            | AlreadyAtLocation { .. }
            | CargoDisabled
            | NoDockInventory { .. }
            | NoVesselCargo { .. }
            | SwapSameCargo { .. }
            | ThroughputExceeded { .. }
            | NegativeTick(_)
            | FutureTick { .. }
            | Overflow(_) => {}
        }
    }
}
