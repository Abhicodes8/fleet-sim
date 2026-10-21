//! Randomized testing: thousands of random command sequences (valid and
//! invalid mixed), with global invariants checked after every command.
//!
//! No external crates: a fixed-seed xorshift generator keeps runs
//! reproducible. A failure prints the seed and step so it can be replayed.

mod common;

use std::collections::HashMap;

use fleet_sim::Simulator;
use fleet_sim::model::{Status, World};
use serde_json::Value;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

const LOCATIONS: usize = 4;
const VESSELS: [&str; 3] = ["a", "b", "c"];

fn init_line(rng: &mut Rng, with_cargo: bool) -> String {
    let mut rows = Vec::new();
    for i in 0..LOCATIONS {
        let row: Vec<String> = (0..LOCATIONS)
            .map(|j| if i == j { 0 } else { 1 + rng.below(4) }.to_string())
            .collect();
        rows.push(format!("[{}]", row.join(",")));
    }
    let vessels: Vec<String> = VESSELS
        .iter()
        .map(|id| {
            format!(
                r#"{{"id":"{id}","location":{}}}"#,
                rng.below(LOCATIONS as u64)
            )
        })
        .collect();
    let cargo = if with_cargo {
        let tp: Vec<String> = (0..LOCATIONS)
            .map(|_| (1 + rng.below(3)).to_string())
            .collect();
        format!(r#","dock_throughput":[{}]"#, tp.join(","))
    } else {
        String::new()
    };
    format!(
        r#"{{"command":"init","parameters":{{"travel_times":[{}],"vessels":[{}]{cargo}}}}}"#,
        rows.join(","),
        vessels.join(",")
    )
}

/// Mostly in-range values, sometimes out of range, to exercise error paths.
fn loc(rng: &mut Rng) -> i64 {
    if rng.chance(5) {
        [-1, LOCATIONS as i64, 99][rng.below(3) as usize]
    } else {
        rng.below(LOCATIONS as u64) as i64
    }
}

fn vessel(rng: &mut Rng) -> &'static str {
    if rng.chance(3) {
        "ghost"
    } else {
        VESSELS[rng.below(VESSELS.len() as u64) as usize]
    }
}

fn manifest(rng: &mut Rng) -> String {
    let n = rng.below(3);
    let entries: Vec<String> = (0..n)
        .map(|_| {
            let q = if rng.chance(5) {
                0
            } else {
                1 + rng.below(3) as i64
            };
            format!(
                r#"{{"location":{},"destination":{},"quantity":{q}}}"#,
                loc(rng),
                loc(rng)
            )
        })
        .collect();
    format!("[{}]", entries.join(","))
}

fn random_command(rng: &mut Rng) -> String {
    match rng.below(100) {
        0..=17 => {
            if rng.chance(40) {
                format!(
                    r#"{{"command":"tick","parameters":{{"cargo_manifest":{}}}}}"#,
                    manifest(rng)
                )
            } else {
                r#"{"command":"tick","parameters":{}}"#.into()
            }
        }
        18..=32 => common::set_dest(vessel(rng), loc(rng)),
        33..=43 => common::dock(vessel(rng)),
        44..=51 => common::undock(vessel(rng)),
        52..=65 => common::load(vessel(rng), loc(rng)),
        66..=77 => common::unload(vessel(rng), loc(rng)),
        78..=85 => common::swap(vessel(rng), loc(rng), loc(rng)),
        86..=89 => r#"{"command":"get_state","parameters":{}}"#.into(),
        90..=94 => common::state_at(rng.below(40) as i64 - 3),
        95 => r#"{"command":"init","parameters":{"travel_times":[[0]],"vessels":[]}}"#.into(),
        96 => "{garbage".into(),
        97 => r#"{"command":"warp"}"#.into(),
        98 => r#"{"command":"load","parameters":{"vessel_id":"a","destination":1.5}}"#.into(),
        _ => r#"{"command":"set_destination","parameters":{}}"#.into(),
    }
}

/// Minimum dock operations implied by the difference between the committed
/// and planned cargo of every vessel docked at `location`.
fn implied_ops(committed: &World, planned: &World, location: usize) -> u64 {
    committed
        .vessels
        .iter()
        .zip(&planned.vessels)
        .filter(|(c, _)| c.status == Status::Docked { location })
        .map(|(c, p)| {
            let (mut loads, mut unloads) = (0u64, 0u64);
            for d in 0..LOCATIONS {
                let diff = p.cargo.count(d) as i64 - c.cargo.count(d) as i64;
                if diff > 0 {
                    loads += diff as u64;
                } else {
                    unloads += diff.unsigned_abs();
                }
            }
            loads.max(unloads)
        })
        .sum()
}

fn check_invariants(sim: &Simulator, frozen: &mut HashMap<usize, World>, ctx: &str) {
    let (Some(committed), Some(planned), Some(history), Some(manifested)) = (
        sim.committed(),
        sim.planned(),
        sim.history(),
        sim.total_manifested(),
    ) else {
        return;
    };

    // History is one snapshot per tick, ending at the committed world.
    assert_eq!(
        history.len() as u64,
        committed.tick + 1,
        "{ctx}: history length"
    );
    assert_eq!(history.last().unwrap(), committed, "{ctx}: history tail");
    for (t, w) in history.iter().enumerate() {
        assert_eq!(w.tick, t as u64, "{ctx}: snapshot tick");
        // Snapshots never change once taken.
        let prev = frozen.entry(t).or_insert_with(|| w.clone());
        assert_eq!(prev, w, "{ctx}: snapshot {t} was mutated");
    }

    // Planned never runs ahead of committed in time.
    assert_eq!(planned.tick, committed.tick, "{ctx}: planned tick");

    // Cargo is never created or destroyed except by manifests and delivery.
    assert_eq!(
        committed.cargo_accounted(),
        manifested,
        "{ctx}: committed cargo"
    );
    assert_eq!(
        planned.cargo_accounted(),
        manifested,
        "{ctx}: planned cargo"
    );

    for (c, p) in committed.vessels.iter().zip(&planned.vessels) {
        // In-transit vessels always arrive in the future.
        if let Status::Transit {
            arrives_at_tick,
            from,
            to,
        } = c.status
        {
            assert!(arrives_at_tick > committed.tick, "{ctx}: stale transit");
            assert_ne!(from, to, "{ctx}: zero-length trip");
            assert_eq!(p.status, c.status, "{ctx}: moving vessel was re-planned");
        }
        // Only docked vessels may change cargo before the next tick.
        if !matches!(c.status, Status::Docked { .. }) {
            assert_eq!(p.cargo, c.cargo, "{ctx}: undocked vessel cargo changed");
        }
    }

    // No dock ever holds cargo bound for itself (it would be delivered).
    for w in [committed, planned] {
        for (i, dock) in w.docks.iter().enumerate() {
            assert_eq!(dock.count(i), 0, "{ctx}: dock {i} holds its own cargo");
        }
    }

    // Planned cargo changes never exceed any dock's throughput.
    if let Some(throughput) = sim.dock_throughput() {
        for (loc, &limit) in throughput.iter().enumerate() {
            let used = implied_ops(committed, planned, loc);
            assert!(used <= limit, "{ctx}: dock {loc} uses {used} > {limit}");
        }
    }
}

fn run_seed(seed: u64, with_cargo: bool, steps: usize) -> Vec<String> {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut sim = Simulator::new();
    let mut frozen = HashMap::new();
    let mut outputs = Vec::with_capacity(steps + 1);

    // Sometimes send a command before init to exercise the lifecycle check.
    if rng.chance(20) {
        let mut cmd = random_command(&mut rng);
        while cmd.contains(r#""init""#) {
            cmd = random_command(&mut rng);
        }
        let r = sim.process(&cmd);
        assert!(
            r.contains(r#""status":"error""#),
            "seed {seed}: pre-init command accepted"
        );
    }
    let init = init_line(&mut rng, with_cargo);
    outputs.push(sim.process(&init));

    for step in 0..steps {
        let cmd = random_command(&mut rng);
        let before = (
            sim.committed().cloned(),
            sim.planned().cloned(),
            sim.history().map(<[_]>::len),
        );
        let out = sim.process(&cmd);
        let ctx = format!("seed {seed} step {step} cmd {cmd}");

        // Exactly one well-formed response per command.
        assert!(!out.contains('\n'), "{ctx}: multi-line response");
        let v: Value = serde_json::from_str(&out).unwrap();
        match v["status"].as_str() {
            Some("ok") => assert!(v["payload"].is_object(), "{ctx}: ok without payload"),
            Some("error") => {
                assert!(
                    v["payload"]["message"].is_string(),
                    "{ctx}: error without message"
                );
                // Errors never change anything.
                let after = (
                    sim.committed().cloned(),
                    sim.planned().cloned(),
                    sim.history().map(<[_]>::len),
                );
                assert_eq!(before, after, "{ctx}: failed command mutated state");
            }
            other => panic!("{ctx}: bad status {other:?}"),
        }
        check_invariants(&sim, &mut frozen, &ctx);
        outputs.push(out);
    }
    outputs
}

#[test]
fn random_sessions_with_cargo_hold_invariants() {
    for seed in 0..400 {
        run_seed(seed, true, 400);
    }
}

#[test]
fn random_sessions_without_cargo_hold_invariants() {
    for seed in 1000..1200 {
        run_seed(seed, false, 300);
    }
}

#[test]
fn simulation_is_deterministic() {
    for seed in [7, 42, 1234] {
        assert_eq!(run_seed(seed, true, 500), run_seed(seed, true, 500));
    }
}
