//! Plain data: the world at one instant, and how it serializes.
//!
//! A [`World`] is a value. The engine keeps three kinds of them — the committed
//! world, the planned world, and one historical snapshot per tick — and moves
//! between them by cloning, which keeps "what changed and when" easy to reason
//! about.

use std::fmt::Write;

use crate::error::{SimError, SimResult};

/// A bag of identical-by-destination cargo units.
///
/// Entries keep the order in which a destination was first added (matching the
/// spec examples, where a vessel that loads destination 2 and then 1 reports
/// `[2, 1]`). A destination whose count reaches zero is removed; if it comes
/// back later it goes to the end.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CargoHold {
    entries: Vec<(usize, u64)>,
}

impl CargoHold {
    pub fn count(&self, destination: usize) -> u64 {
        self.entries
            .iter()
            .find(|(d, _)| *d == destination)
            .map_or(0, |(_, q)| *q)
    }

    pub fn total(&self) -> u64 {
        self.entries.iter().map(|(_, q)| q).sum()
    }

    pub fn add(&mut self, destination: usize, quantity: u64) -> SimResult<()> {
        if quantity == 0 {
            return Ok(());
        }
        match self.entries.iter_mut().find(|(d, _)| *d == destination) {
            Some((_, q)) => {
                *q = q
                    .checked_add(quantity)
                    .ok_or(SimError::Overflow("cargo quantity"))?;
            }
            None => self.entries.push((destination, quantity)),
        }
        Ok(())
    }

    /// Removes one unit. Returns `false` (and changes nothing) if none exist.
    pub fn remove_one(&mut self, destination: usize) -> bool {
        let Some(pos) = self.entries.iter().position(|(d, _)| *d == destination) else {
            return false;
        };
        self.entries[pos].1 -= 1;
        if self.entries[pos].1 == 0 {
            self.entries.remove(pos);
        }
        true
    }

    /// Appends `[{"destination":d,"quantity":q},...]`.
    fn write_json(&self, out: &mut String) {
        out.push('[');
        for (k, (d, q)) in self.entries.iter().enumerate() {
            if k > 0 {
                out.push(',');
            }
            // Writing to a String cannot fail.
            let _ = write!(out, r#"{{"destination":{d},"quantity":{q}}}"#);
        }
        out.push(']');
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle {
        location: usize,
    },
    Transit {
        from: usize,
        to: usize,
        arrives_at_tick: u64,
    },
    Docked {
        location: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vessel {
    pub id: String,
    pub status: Status,
    pub cargo: CargoHold,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    pub tick: u64,
    /// In `init` order, which is also the order they are reported in.
    pub vessels: Vec<Vessel>,
    /// One inventory per location.
    pub docks: Vec<CargoHold>,
    /// Units unloaded at their destination and removed from the simulation.
    /// Not part of the wire format; kept so cargo conservation can be checked.
    pub delivered: u64,
}

impl World {
    /// Serialize as the spec's `WorldState`.
    ///
    /// With cargo disabled (base problem) the `cargo` and `docks` fields are
    /// omitted so output matches the base examples exactly. Field order follows
    /// the spec: `tick`, `vessels` (in init order), then `docks` by location.
    ///
    /// Every response carries a full world, so this is the hot path; it writes
    /// straight into a `String` instead of building a `serde_json::Value` tree.
    pub fn to_json_string(&self, cargo_enabled: bool) -> String {
        let mut out = String::with_capacity(64 + 96 * self.vessels.len());
        let _ = write!(out, r#"{{"tick":{},"vessels":{{"#, self.tick);
        for (k, v) in self.vessels.iter().enumerate() {
            if k > 0 {
                out.push(',');
            }
            write_json_str(&mut out, &v.id);
            let _ = match v.status {
                Status::Idle { location } => {
                    write!(out, r#":{{"state":"idle","location":{location}"#)
                }
                Status::Transit {
                    from,
                    to,
                    arrives_at_tick,
                } => write!(
                    out,
                    r#":{{"state":"transit","from":{from},"to":{to},"arrives_at_tick":{arrives_at_tick}"#
                ),
                Status::Docked { location } => {
                    write!(out, r#":{{"state":"docked","location":{location}"#)
                }
            };
            if cargo_enabled {
                out.push_str(r#","cargo":"#);
                v.cargo.write_json(&mut out);
            }
            out.push('}');
        }
        out.push('}');
        if cargo_enabled {
            out.push_str(r#","docks":{"#);
            for (i, dock) in self.docks.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let _ = write!(out, r#""{i}":{{"cargo":"#);
                dock.write_json(&mut out);
                out.push('}');
            }
            out.push('}');
        }
        out.push('}');
        out
    }

    /// Every unit currently in the world, plus everything delivered.
    pub fn cargo_accounted(&self) -> u64 {
        let docks: u64 = self.docks.iter().map(CargoHold::total).sum();
        let vessels: u64 = self.vessels.iter().map(|v| v.cargo.total()).sum();
        docks + vessels + self.delivered
    }
}

/// Appends `s` as a JSON string literal, escaped exactly as serde_json does.
fn write_json_str(out: &mut String, s: &str) {
    out.push_str(&serde_json::Value::from(s).to_string());
}
