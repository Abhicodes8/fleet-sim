//! Behavioral rules: tick semantics, committed vs planned, every
//! "left to the implementer" decision, throughput, delivery, history.

mod common;

use common::*;
use serde_json::json;

// ---- Committed vs planned ----------------------------------------------------

#[test]
fn commands_change_planned_not_committed() {
    let mut s = Session::base();
    s.ok(&set_dest("v1", 1));
    assert_eq!(s.state()["vessels"]["v1"]["state"], "transit");
    // History (committed) still shows tick 0 idle.
    assert_eq!(s.ok(&state_at(0))["vessels"]["v1"]["state"], "idle");
}

#[test]
fn latest_set_destination_wins() {
    let mut s = Session::base();
    s.ok(&set_dest("v1", 2));
    s.ok(&set_dest("v1", 1));
    let p = s.tick();
    assert_eq!(p["vessels"]["v1"]["to"], 1);
    assert_eq!(p["vessels"]["v1"]["arrives_at_tick"], 4);
}

#[test]
fn get_state_is_read_only() {
    let mut s = Session::base();
    s.ok(&set_dest("v1", 1));
    let a = s.state();
    let b = s.state();
    assert_eq!(a, b);
}

#[test]
fn tick_returns_new_committed_state() {
    let mut s = Session::base();
    s.ok(&set_dest("v1", 1));
    let ticked = s.tick();
    assert_eq!(ticked, s.state());
    assert_eq!(ticked, s.ok(&state_at(1)));
}

// ---- Tick semantics ----------------------------------------------------------

#[test]
fn arrival_reports_idle_on_arrival_tick() {
    let mut s = Session::base();
    s.ok(&set_dest("v1", 1)); // issued at 0, arrives at 4
    for t in 1..=3 {
        let p = s.tick();
        assert_eq!(p["tick"], t);
        assert_eq!(p["vessels"]["v1"]["state"], "transit");
    }
    let p = s.tick();
    assert_eq!(p["tick"], 4);
    assert_eq!(p["vessels"]["v1"], json!({"state":"idle","location":1}));
}

#[test]
fn arrived_vessel_can_be_commanded_same_tick() {
    let mut s = Session::base();
    s.ok(&set_dest("v1", 1));
    s.ticks(4);
    let p = s.ok(&set_dest("v1", 2));
    assert_eq!(
        p["vessels"]["v1"],
        json!({"state":"transit","from":1,"to":2,"arrives_at_tick":7})
    );
}

#[test]
fn arrival_time_uses_issue_tick() {
    let mut s = Session::base();
    s.ticks(10);
    let p = s.ok(&set_dest("v2", 0)); // 2 -> 0 takes 7
    assert_eq!(p["vessels"]["v2"]["arrives_at_tick"], 17);
}

#[test]
fn one_tick_trip() {
    let mut s = Session::new();
    s.ok(r#"{"command":"init","parameters":{"travel_times":[[0,1],[1,0]],"vessels":[{"id":"a","location":0}]}}"#);
    s.ok(&set_dest("a", 1));
    let p = s.tick();
    assert_eq!(p["vessels"]["a"], json!({"state":"idle","location":1}));
}

#[test]
fn vessels_report_in_init_order() {
    let mut s = Session::new();
    let line = s.sim.process(r#"{"command":"init","parameters":{"travel_times":[[0]],"vessels":[{"id":"zulu","location":0},{"id":"alpha","location":0},{"id":"v10","location":0},{"id":"v2","location":0}]}}"#);
    // Check the raw text: a parsed JSON map would not preserve key order.
    let pos = |id: &str| line.find(&format!(r#""{id}":"#)).unwrap();
    assert!(pos("zulu") < pos("alpha") && pos("alpha") < pos("v10") && pos("v10") < pos("v2"));
}

#[test]
fn empty_fleet_is_valid() {
    let mut s = Session::new();
    let p = s.ok(r#"{"command":"init","parameters":{"travel_times":[[0]],"vessels":[]}}"#);
    assert_eq!(p, json!({"tick":0,"vessels":{}}));
    assert_eq!(s.tick(), json!({"tick":1,"vessels":{}}));
}

#[test]
fn parameters_may_be_omitted_when_empty() {
    let mut s = Session::base();
    assert_eq!(s.ok(r#"{"command":"tick"}"#)["tick"], 1);
    assert_eq!(
        s.ok(r#"{"command":"get_state","parameters":null}"#)["tick"],
        1
    );
}

// ---- Implementer decisions: movement -----------------------------------------

/// Decision: set_destination to the current location is rejected.
#[test]
fn set_destination_to_current_location_rejected() {
    let mut s = Session::base();
    s.err_unchanged(&set_dest("v1", 0));
}

/// Decision: dock then set_destination, or set_destination then dock — the
/// latest wins (both are movement intents for an idle vessel).
#[test]
fn dock_and_set_destination_overwrite_each_other() {
    let mut c = Session::cargo();
    c.ok(&set_dest("v1", 1));
    assert_eq!(c.ok(&dock("v1"))["vessels"]["v1"]["state"], "docked");
    assert_eq!(
        c.ok(&set_dest("v1", 2))["vessels"]["v1"]["state"],
        "transit"
    );
    assert_eq!(c.tick()["vessels"]["v1"]["state"], "transit");
}

/// Decision: dock after dock is an idempotent success.
#[test]
fn dock_after_dock_is_idempotent() {
    let mut c = Session::cargo();
    let a = c.ok(&dock("v1"));
    let b = c.ok(&dock("v1"));
    assert_eq!(a, b);
}

/// Decision: undock after a pending dock is rejected — undock requires the
/// *committed* state to be docked, and it is still idle.
#[test]
fn undock_after_pending_dock_rejected() {
    let mut c = Session::cargo();
    c.ok(&dock("v1"));
    assert_eq!(c.err_unchanged(&undock("v1")), "vessel v1 is not docked");
}

/// Decision: undock after undock is an idempotent success.
#[test]
fn undock_after_undock_is_idempotent() {
    let mut c = Session::cargo();
    c.ok(&dock("v1"));
    c.tick();
    let a = c.ok(&undock("v1"));
    let b = c.ok(&undock("v1"));
    assert_eq!(a, b);
}

/// Decision: dock after a pending undock is rejected — dock requires the
/// committed state to be idle, and it is still docked.
#[test]
fn dock_after_pending_undock_rejected() {
    let mut c = Session::cargo();
    c.ok(&dock("v1"));
    c.tick();
    c.ok(&undock("v1"));
    assert_eq!(c.err_unchanged(&dock("v1")), "vessel v1 is not idle");
}

// ---- Implementer decisions: cargo + undock ------------------------------------

fn docked_with_cargo_at_0() -> Session {
    let mut c = Session::cargo();
    c.manifest_tick(r#"[{"location":0,"destination":2,"quantity":3},{"location":0,"destination":1,"quantity":3}]"#);
    c.ok(&dock("v1"));
    c.tick();
    c
}

/// Decision: undock after load is allowed — the cargo work happens at the
/// dock and the vessel leaves when the tick commits.
#[test]
fn undock_after_load_allowed() {
    let mut c = docked_with_cargo_at_0();
    c.ok(&load("v1", 2));
    let p = c.ok(&undock("v1"));
    assert_eq!(p["vessels"]["v1"]["state"], "idle");
    assert_eq!(cargo(&p["vessels"]["v1"]["cargo"]), vec![(2, 1)]);
    let p = c.tick();
    assert_eq!(p["vessels"]["v1"]["state"], "idle");
    assert_eq!(cargo(&p["vessels"]["v1"]["cargo"]), vec![(2, 1)]);
}

/// Decision: load after undock is allowed, symmetric with the above.
#[test]
fn load_after_undock_allowed() {
    let mut c = docked_with_cargo_at_0();
    c.ok(&undock("v1"));
    let p = c.ok(&load("v1", 1));
    assert_eq!(p["vessels"]["v1"]["state"], "idle");
    assert_eq!(cargo(&p["vessels"]["v1"]["cargo"]), vec![(1, 1)]);
}

/// Both orders produce the same committed world.
#[test]
fn undock_and_load_commute() {
    let mut a = docked_with_cargo_at_0();
    a.ok(&load("v1", 2));
    a.ok(&undock("v1"));
    let mut b = docked_with_cargo_at_0();
    b.ok(&undock("v1"));
    b.ok(&load("v1", 2));
    assert_eq!(a.tick(), b.tick());
}

// ---- Implementer decisions: throughput accounting -----------------------------

/// Decision: load then unload of the same cargo cancels out and costs nothing,
/// because the pending state equals the committed state.
#[test]
fn load_then_unload_same_cargo_cancels() {
    let mut c = docked_with_cargo_at_0();
    let before = c.state();
    c.ok(&load("v1", 2));
    c.ok(&unload("v1", 2));
    assert_eq!(c.state(), before);
    // Full throughput (2) is still available.
    c.ok(&load("v1", 2));
    c.ok(&load("v1", 1));
}

/// A load and an unload of different cargo is achievable as one swap, so it
/// is charged as one operation.
#[test]
fn separate_load_and_unload_cost_one_swap() {
    let mut c = docked_with_cargo_at_0();
    c.ok(&load("v1", 2));
    c.tick();
    // throughput at 0 is 2
    c.ok(&load("v1", 1)); // cost 1
    c.ok(&unload("v1", 2)); // load 1 + unload 2 = one swap, cost still 1
    c.ok(&load("v1", 1)); // cost 2
    c.err_unchanged(&load("v1", 1)); // cost 3 > 2
}

#[test]
fn swap_costs_one_operation() {
    let mut c = docked_with_cargo_at_0();
    c.ok(&load("v1", 2));
    c.ok(&load("v1", 2));
    c.tick();
    c.ok(&load("v1", 2));
    c.tick();
    // Vessel holds 3 x dest-2, dock holds 3 x dest-1; throughput is 2.
    c.ok(&swap("v1", 1, 2));
    c.ok(&swap("v1", 1, 2));
    assert_eq!(
        c.err_unchanged(&swap("v1", 1, 2)),
        "transfer of 3 operations exceeds dock throughput of 2 at location 0"
    );
}

#[test]
fn throughput_is_shared_across_vessels_at_a_dock() {
    let mut c = Session::new();
    c.ok(r#"{"command":"init","parameters":{"travel_times":[[0,1],[1,0]],"dock_throughput":[3,1],"vessels":[{"id":"a","location":0},{"id":"b","location":0}],"cargo_manifest":[{"location":0,"destination":1,"quantity":10}]}}"#);
    c.ok(&dock("a"));
    c.ok(&dock("b"));
    c.tick();
    c.ok(&load("a", 1));
    c.ok(&load("b", 1));
    c.ok(&load("a", 1));
    assert_eq!(
        c.err_unchanged(&load("b", 1)),
        "transfer of 4 operations exceeds dock throughput of 3 at location 0"
    );
}

#[test]
fn throughput_is_per_dock() {
    let mut c = Session::new();
    c.ok(r#"{"command":"init","parameters":{"travel_times":[[0,1],[1,0]],"dock_throughput":[1,1],"vessels":[{"id":"a","location":0},{"id":"b","location":1}],"cargo_manifest":[{"location":0,"destination":1,"quantity":5},{"location":1,"destination":0,"quantity":5}]}}"#);
    c.ok(&dock("a"));
    c.ok(&dock("b"));
    c.tick();
    c.ok(&load("a", 1));
    c.ok(&load("b", 0)); // different dock, own budget
}

#[test]
fn throughput_resets_every_tick() {
    let mut c = docked_with_cargo_at_0();
    c.ok(&load("v1", 2));
    c.ok(&load("v1", 2));
    c.tick();
    c.ok(&load("v1", 2));
    c.ok(&load("v1", 1));
}

// ---- Inventory ----------------------------------------------------------------

#[test]
fn inventory_checks_use_planned_state() {
    let mut c = Session::new();
    c.ok(r#"{"command":"init","parameters":{"travel_times":[[0,1],[1,0]],"dock_throughput":[5,5],"vessels":[{"id":"a","location":0},{"id":"b","location":0}],"cargo_manifest":[{"location":0,"destination":1,"quantity":1}]}}"#);
    c.ok(&dock("a"));
    c.ok(&dock("b"));
    c.tick();
    c.ok(&load("a", 1));
    // The only unit is already spoken for in the planned state.
    assert_eq!(
        c.err_unchanged(&load("b", 1)),
        "dock at location 0 has no cargo for destination 1"
    );
    // a puts it back, b can take it.
    c.ok(&unload("a", 1));
    c.ok(&load("b", 1));
}

#[test]
fn unload_at_destination_delivers() {
    let mut c = Session::new();
    c.ok(r#"{"command":"init","parameters":{"travel_times":[[0,1],[1,0]],"dock_throughput":[5,5],"vessels":[{"id":"a","location":0}],"cargo_manifest":[{"location":0,"destination":1,"quantity":2}]}}"#);
    c.ok(&dock("a"));
    c.tick();
    c.ok(&load("a", 1));
    c.ok(&load("a", 1));
    c.tick();
    c.ok(&undock("a"));
    c.tick();
    c.ok(&set_dest("a", 1));
    c.tick();
    c.ok(&dock("a"));
    c.tick();
    let p = c.ok(&unload("a", 1));
    assert_eq!(cargo(&p["vessels"]["a"]["cargo"]), vec![(1, 1)]);
    assert_eq!(cargo(&p["docks"]["1"]["cargo"]), vec![]); // gone, not stored
    assert_eq!(c.sim.planned().unwrap().delivered, 1);
}

#[test]
fn unload_elsewhere_stores_at_dock() {
    let mut c = docked_with_cargo_at_0();
    c.ok(&load("v1", 2));
    c.tick();
    let p = c.ok(&unload("v1", 2));
    assert_eq!(cargo(&p["docks"]["0"]["cargo"]), vec![(2, 3), (1, 3)]);
}

#[test]
fn swap_checks_both_sides_before_moving() {
    let mut c = docked_with_cargo_at_0();
    c.ok(&load("v1", 2));
    c.tick();
    // Dock 0 has 2 x dest-2 and 3 x dest-1; vessel has 1 x dest-2.
    // swap(load 1, unload 1): rejected as same-cargo. swap(load 2, unload 1):
    // vessel has no dest-1 cargo yet -> rejected, nothing moves.
    c.err_unchanged(&swap("v1", 2, 1));
    let p = c.ok(&swap("v1", 1, 2));
    assert_eq!(cargo(&p["vessels"]["v1"]["cargo"]), vec![(1, 1)]);
}

#[test]
fn cargo_order_is_first_added() {
    let mut c = docked_with_cargo_at_0();
    let p = c.ok(&load("v1", 2));
    let p2 = c.ok(&load("v1", 1));
    assert_eq!(cargo(&p["vessels"]["v1"]["cargo"]), vec![(2, 1)]);
    assert_eq!(cargo(&p2["vessels"]["v1"]["cargo"]), vec![(2, 1), (1, 1)]);
}

#[test]
fn manifest_on_init_and_tick() {
    let mut c = Session::new();
    let p = c.ok(r#"{"command":"init","parameters":{"travel_times":[[0,1],[1,0]],"dock_throughput":[1,1],"vessels":[],"cargo_manifest":[{"location":0,"destination":1,"quantity":2},{"location":0,"destination":1,"quantity":3}]}}"#);
    assert_eq!(cargo(&p["docks"]["0"]["cargo"]), vec![(1, 5)]);
    let p = c.manifest_tick(r#"[{"location":1,"destination":0,"quantity":4}]"#);
    assert_eq!(cargo(&p["docks"]["1"]["cargo"]), vec![(0, 4)]);
}

#[test]
fn manifest_cargo_lands_in_committed_state() {
    let mut c = Session::cargo();
    c.manifest_tick(r#"[{"location":0,"destination":1,"quantity":1}]"#);
    let at1 = c.ok(&state_at(1));
    assert_eq!(cargo(&at1["docks"]["0"]["cargo"]), vec![(1, 1)]);
}

// ---- Atomicity ----------------------------------------------------------------

#[test]
fn bad_manifest_rejects_whole_tick() {
    let mut c = Session::cargo();
    c.ok(&set_dest("v1", 1));
    let msg = c.err_unchanged(
        r#"{"command":"tick","parameters":{"cargo_manifest":[{"location":0,"destination":1,"quantity":1},{"location":0,"destination":1,"quantity":-2}]}}"#,
    );
    assert!(msg.contains("cargo_manifest[1].quantity"));
    assert_eq!(c.state()["tick"], 0);
    // The pending set_destination survived the rejected tick.
    assert_eq!(c.tick()["vessels"]["v1"]["state"], "transit");
}

#[test]
fn rejected_throughput_leaves_inventory_untouched() {
    let mut c = docked_with_cargo_at_0();
    c.ok(&load("v1", 2));
    c.ok(&load("v1", 2));
    c.err_unchanged(&load("v1", 1));
}

// ---- History ------------------------------------------------------------------

#[test]
fn history_is_immutable() {
    let mut c = docked_with_cargo_at_0();
    let at2 = c.ok(&state_at(2));
    c.ok(&load("v1", 2));
    c.tick();
    c.ok(&undock("v1"));
    c.tick();
    assert_eq!(c.ok(&state_at(2)), at2);
}

#[test]
fn history_excludes_intra_tick_state() {
    let mut s = Session::base();
    s.ok(&set_dest("v1", 1));
    // Current tick is 0; its committed state has v1 idle.
    assert_eq!(s.ok(&state_at(0))["vessels"]["v1"]["state"], "idle");
}

#[test]
fn every_tick_is_queryable() {
    let mut s = Session::base();
    s.ok(&set_dest("v1", 2));
    let mut seen = vec![s.ok(&state_at(0))];
    for _ in 0..10 {
        seen.push(s.tick());
    }
    for (t, snapshot) in seen.iter().enumerate() {
        assert_eq!(&s.ok(&state_at(t as i64)), snapshot);
    }
}

#[test]
fn history_queries_do_not_change_state() {
    let mut s = Session::base();
    s.ok(&set_dest("v1", 1));
    let before = s.state();
    s.ok(&state_at(0));
    s.err(&state_at(5));
    assert_eq!(s.state(), before);
}
