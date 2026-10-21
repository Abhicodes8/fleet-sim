#![allow(dead_code)]

use fleet_sim::Simulator;
use serde_json::Value;

/// Thin test driver around [`Simulator`] that speaks raw JSON lines.
pub struct Session {
    pub sim: Simulator,
}

impl Session {
    pub fn new() -> Self {
        Self {
            sim: Simulator::new(),
        }
    }

    /// Base-problem world from the spec: v1 at 0, v2 at 2.
    pub fn base() -> Self {
        let mut s = Self::new();
        s.ok(r#"{"command":"init","parameters":{"travel_times":[[0,4,7],[4,0,3],[7,3,0]],"vessels":[{"id":"v1","location":0},{"id":"v2","location":2}]}}"#);
        s
    }

    /// Follow-on 1 world from the spec: same map, throughput [2,4,1].
    pub fn cargo() -> Self {
        let mut s = Self::new();
        s.ok(r#"{"command":"init","parameters":{"travel_times":[[0,4,7],[4,0,3],[7,3,0]],"dock_throughput":[2,4,1],"vessels":[{"id":"v1","location":0},{"id":"v2","location":2}]}}"#);
        s
    }

    /// Send a line, return the parsed response.
    pub fn send(&mut self, line: &str) -> Value {
        serde_json::from_str(&self.sim.process(line)).expect("response is always valid JSON")
    }

    /// Send a line that must succeed; return its payload.
    #[track_caller]
    pub fn ok(&mut self, line: &str) -> Value {
        let r = self.send(line);
        assert_eq!(r["status"], "ok", "expected ok for {line}, got {r}");
        r["payload"].clone()
    }

    /// Send a line that must fail; return its message.
    #[track_caller]
    pub fn err(&mut self, line: &str) -> String {
        let r = self.send(line);
        assert_eq!(r["status"], "error", "expected error for {line}, got {r}");
        r["payload"]["message"].as_str().unwrap().to_string()
    }

    /// Send a line that must fail without changing anything observable.
    #[track_caller]
    pub fn err_unchanged(&mut self, line: &str) -> String {
        let before = (
            self.sim.committed().cloned(),
            self.sim.planned().cloned(),
            self.sim.history().map(<[_]>::to_vec),
        );
        let msg = self.err(line);
        let after = (
            self.sim.committed().cloned(),
            self.sim.planned().cloned(),
            self.sim.history().map(<[_]>::to_vec),
        );
        assert_eq!(before, after, "failed command {line} mutated state");
        msg
    }

    // Shorthands for common commands.

    pub fn tick(&mut self) -> Value {
        self.ok(r#"{"command":"tick","parameters":{}}"#)
    }

    pub fn ticks(&mut self, n: usize) -> Value {
        let mut last = Value::Null;
        for _ in 0..n {
            last = self.tick();
        }
        last
    }

    pub fn state(&mut self) -> Value {
        self.ok(r#"{"command":"get_state","parameters":{}}"#)
    }

    pub fn manifest_tick(&mut self, entries: &str) -> Value {
        self.ok(&format!(
            r#"{{"command":"tick","parameters":{{"cargo_manifest":{entries}}}}}"#
        ))
    }
}

pub fn set_dest(v: &str, d: i64) -> String {
    format!(
        r#"{{"command":"set_destination","parameters":{{"vessel_id":"{v}","destination":{d}}}}}"#
    )
}

pub fn dock(v: &str) -> String {
    format!(r#"{{"command":"dock","parameters":{{"vessel_id":"{v}"}}}}"#)
}

pub fn undock(v: &str) -> String {
    format!(r#"{{"command":"undock","parameters":{{"vessel_id":"{v}"}}}}"#)
}

pub fn load(v: &str, d: i64) -> String {
    format!(r#"{{"command":"load","parameters":{{"vessel_id":"{v}","destination":{d}}}}}"#)
}

pub fn unload(v: &str, d: i64) -> String {
    format!(r#"{{"command":"unload","parameters":{{"vessel_id":"{v}","destination":{d}}}}}"#)
}

pub fn swap(v: &str, l: i64, u: i64) -> String {
    format!(
        r#"{{"command":"swap","parameters":{{"vessel_id":"{v}","load_destination":{l},"unload_destination":{u}}}}}"#
    )
}

pub fn state_at(t: i64) -> String {
    format!(r#"{{"command":"get_state_at","parameters":{{"tick":{t}}}}}"#)
}

/// `[{"destination":d,"quantity":q}, ...]` as `(d, q)` pairs.
pub fn cargo(v: &Value) -> Vec<(u64, u64)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["destination"].as_u64().unwrap(),
                e["quantity"].as_u64().unwrap(),
            )
        })
        .collect()
}
