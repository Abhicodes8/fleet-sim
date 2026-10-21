//! The simulation engine.
//!
//! ## State
//!
//! * `committed` — the world as of the last tick. Every validity check
//!   ("is the vessel idle?", "is it docked?") reads this.
//! * `pending` — per vessel, the net cargo work queued since the last tick.
//! * `planned` — `committed` with `pending` applied. Updated incrementally by
//!   every accepted command; inventory checks read this.
//! * `history` — `history[t]` is the committed world at tick `t`.
//!
//! ## The one rule that answers most edge cases
//!
//! A command is *allowed* based on the committed state and *takes effect* in
//! the planned state. Movement intents for the same vessel replace each other;
//! cargo operations accumulate. On `tick`, planned becomes committed.
//!
//! ## Atomicity
//!
//! Every handler finishes all of its checks before its first write, and `tick`
//! builds the next world in a scratch value that is only installed on success.
//! So an error response always means the simulation is exactly as it was.

use std::collections::HashMap;

use crate::error::{SimError, SimResult};
use crate::model::{CargoHold, Status, Vessel, World};
use crate::protocol::{
    Command, CommandKind, InitParams, ManifestEntry, error_response, ok_response, parse_command,
    parse_envelope,
};

/// Cargo work a vessel has queued since the last tick.
///
/// (A vessel's pending *movement* needs no separate record: it is fully
/// described by its planned status, and a new movement command simply
/// recomputes that status from the committed one, which is what makes
/// "the latest command wins" automatic.)
#[derive(Debug, Clone)]
struct Pending {
    /// `net[d]` = loads − unloads of destination `d` this tick.
    net: Vec<i64>,
}

impl Pending {
    fn new(locations: usize) -> Self {
        Self {
            net: vec![0; locations],
        }
    }

    /// Fewest dock operations that turn the committed cargo into the planned
    /// cargo. A load and an unload of the same destination cancel out; a load
    /// and an unload of different destinations can share one `swap`. So the
    /// cost is max(net loads, net unloads).
    fn transfer_cost(&self) -> u64 {
        let (loads, unloads) = self.net.iter().fold((0u64, 0u64), |(l, u), &n| {
            if n > 0 {
                (l + n as u64, u)
            } else {
                (l, u + n.unsigned_abs())
            }
        });
        loads.max(unloads)
    }
}

/// An initialized simulation.
struct Engine {
    travel_times: Vec<Vec<u64>>,
    /// `None` = base problem (no cargo); `Some` = follow-on 1 enabled.
    dock_throughput: Option<Vec<u64>>,
    vessel_index: HashMap<String, usize>,
    committed: World,
    planned: World,
    pending: Vec<Pending>,
    history: Vec<World>,
    /// Total units ever added by manifests (for conservation checks).
    manifested: u64,
}

impl Engine {
    // ---------------------------------------------------------------- init --

    fn init(p: InitParams) -> SimResult<Self> {
        let travel_times = validate_travel_times(&p.travel_times)?;
        let n = travel_times.len();

        let dock_throughput = p
            .dock_throughput
            .as_deref()
            .map(|t| validate_throughput(t, n))
            .transpose()?;

        let mut vessel_index = HashMap::with_capacity(p.vessels.len());
        let mut vessels = Vec::with_capacity(p.vessels.len());
        for (i, spec) in p.vessels.iter().enumerate() {
            if spec.id.is_empty() {
                return Err(SimError::EmptyVesselId { index: i });
            }
            let location = check_location(spec.location, &format!("vessels[{i}].location"), n)?;
            if vessel_index.insert(spec.id.clone(), i).is_some() {
                return Err(SimError::DuplicateVessel(spec.id.clone()));
            }
            vessels.push(Vessel {
                id: spec.id.clone(),
                status: Status::Idle { location },
                cargo: CargoHold::default(),
            });
        }

        let mut world = World {
            tick: 0,
            vessels,
            docks: vec![CargoHold::default(); n],
            delivered: 0,
        };

        let mut manifested = 0;
        if let Some(entries) = &p.cargo_manifest {
            if dock_throughput.is_none() {
                return Err(SimError::CargoDisabled);
            }
            manifested = apply_manifest(&mut world, &validate_manifest(entries, n)?)?;
        }

        Ok(Self {
            pending: vec![Pending::new(n); world.vessels.len()],
            travel_times,
            dock_throughput,
            vessel_index,
            committed: world.clone(),
            planned: world.clone(),
            history: vec![world],
            manifested,
        })
    }

    // ------------------------------------------------------------ dispatch --

    /// Runs one command; returns the serialized payload.
    fn apply(&mut self, cmd: Command) -> SimResult<String> {
        match cmd {
            Command::Init(_) => return Err(SimError::AlreadyInitialized),
            Command::Tick { cargo_manifest } => self.tick(cargo_manifest.as_deref())?,
            Command::SetDestination {
                vessel_id,
                destination,
            } => self.set_destination(&vessel_id, destination)?,
            Command::Dock { vessel_id } => self.dock(&vessel_id)?,
            Command::Undock { vessel_id } => self.undock(&vessel_id)?,
            Command::Load {
                vessel_id,
                destination,
            } => self.transfer(&vessel_id, Some((destination, "destination")), None)?,
            Command::Unload {
                vessel_id,
                destination,
            } => self.transfer(&vessel_id, None, Some((destination, "destination")))?,
            Command::Swap {
                vessel_id,
                load_destination,
                unload_destination,
            } => self.transfer(
                &vessel_id,
                Some((load_destination, "load_destination")),
                Some((unload_destination, "unload_destination")),
            )?,
            Command::GetState => {}
            Command::GetStateAt { tick } => return self.state_at(tick),
        }
        Ok(self.planned_json())
    }

    fn cargo_enabled(&self) -> bool {
        self.dock_throughput.is_some()
    }

    fn planned_json(&self) -> String {
        self.planned.to_json_string(self.cargo_enabled())
    }

    fn locations(&self) -> usize {
        self.travel_times.len()
    }

    fn vessel(&self, id: &str) -> SimResult<usize> {
        self.vessel_index
            .get(id)
            .copied()
            .ok_or_else(|| SimError::UnknownVessel(id.to_string()))
    }

    fn throughput(&self) -> SimResult<&[u64]> {
        self.dock_throughput
            .as_deref()
            .ok_or(SimError::CargoDisabled)
    }

    // ---------------------------------------------------------------- tick --

    fn tick(&mut self, manifest: Option<&[ManifestEntry]>) -> SimResult<()> {
        // Validate everything up front: a bad manifest rejects the whole tick.
        let manifest = match manifest {
            Some(entries) => {
                self.throughput()?;
                validate_manifest(entries, self.locations())?
            }
            None => Vec::new(),
        };

        // Build the next world in scratch space.
        let mut next = self.planned.clone();
        next.tick = next.tick.checked_add(1).ok_or(SimError::Overflow("tick"))?;
        for v in &mut next.vessels {
            // A vessel arriving at tick T reports idle at T.
            if let Status::Transit {
                to,
                arrives_at_tick,
                ..
            } = v.status
                && arrives_at_tick <= next.tick
            {
                v.status = Status::Idle { location: to };
            }
        }
        let added = apply_manifest(&mut next, &manifest)?;
        let manifested = self
            .manifested
            .checked_add(added)
            .ok_or(SimError::Overflow("cargo total"))?;

        // Install. Nothing below can fail.
        self.manifested = manifested;
        self.history.push(next.clone());
        self.committed = next.clone();
        self.planned = next;
        let n = self.locations();
        self.pending.iter_mut().for_each(|p| *p = Pending::new(n));
        Ok(())
    }

    // ------------------------------------------------------------ movement --

    fn set_destination(&mut self, id: &str, destination: i64) -> SimResult<()> {
        let i = self.vessel(id)?;
        let to = check_location(destination, "destination", self.locations())?;
        let Status::Idle { location: from } = self.committed.vessels[i].status else {
            return Err(SimError::NotIdle(id.to_string()));
        };
        if to == from {
            return Err(SimError::AlreadyAtLocation {
                vessel: id.to_string(),
                location: from,
            });
        }
        let arrives_at_tick = self
            .committed
            .tick
            .checked_add(self.travel_times[from][to])
            .ok_or(SimError::Overflow("arrival tick"))?;

        self.planned.vessels[i].status = Status::Transit {
            from,
            to,
            arrives_at_tick,
        };
        Ok(())
    }

    fn dock(&mut self, id: &str) -> SimResult<()> {
        self.throughput()?;
        let i = self.vessel(id)?;
        let Status::Idle { location } = self.committed.vessels[i].status else {
            return Err(SimError::NotIdle(id.to_string()));
        };
        // Replaces any pending set_destination; dock after dock is a no-op.
        self.planned.vessels[i].status = Status::Docked { location };
        Ok(())
    }

    fn undock(&mut self, id: &str) -> SimResult<()> {
        self.throughput()?;
        let i = self.vessel(id)?;
        let Status::Docked { location } = self.committed.vessels[i].status else {
            return Err(SimError::NotDocked(id.to_string()));
        };
        // Undock after undock is a no-op. Pending cargo ops are kept: they are
        // carried out at the dock before the vessel leaves on the next tick.
        self.planned.vessels[i].status = Status::Idle { location };
        Ok(())
    }

    // --------------------------------------------------------------- cargo --

    /// `load`, `unload` and `swap` share one code path: a swap is simply a load
    /// and an unload checked and applied together. Each side carries its
    /// parameter name for error messages.
    fn transfer(
        &mut self,
        id: &str,
        load: Option<(i64, &str)>,
        unload: Option<(i64, &str)>,
    ) -> SimResult<()> {
        let n = self.locations();
        self.throughput()?;
        let i = self.vessel(id)?;
        let load = load
            .map(|(d, field)| check_location(d, field, n))
            .transpose()?;
        let unload = unload
            .map(|(d, field)| check_location(d, field, n))
            .transpose()?;
        let Status::Docked { location } = self.committed.vessels[i].status else {
            return Err(SimError::NotDocked(id.to_string()));
        };
        if let (Some(l), Some(u)) = (load, unload)
            && l == u
        {
            return Err(SimError::SwapSameCargo {
                vessel: id.to_string(),
                destination: l,
            });
        }

        // Inventory, against the planned state. A swap is simultaneous, so both
        // sides are checked against the state *before* the swap.
        if let Some(d) = load
            && self.planned.docks[location].count(d) == 0
        {
            return Err(SimError::NoDockInventory {
                location,
                destination: d,
            });
        }
        if let Some(d) = unload
            && self.planned.vessels[i].cargo.count(d) == 0
        {
            return Err(SimError::NoVesselCargo {
                vessel: id.to_string(),
                destination: d,
            });
        }

        // Throughput: total cost of every vessel docked here, with this vessel's
        // tally as it would be after the command.
        let mut tally = self.pending[i].clone();
        if let Some(d) = load {
            tally.net[d] += 1;
        }
        if let Some(d) = unload {
            tally.net[d] -= 1;
        }
        let requested: u64 = self
            .committed
            .vessels
            .iter()
            .enumerate()
            .filter(|(_, v)| v.status == Status::Docked { location })
            .map(|(j, _)| {
                if j == i {
                    tally.transfer_cost()
                } else {
                    self.pending[j].transfer_cost()
                }
            })
            .sum();
        let limit = self.throughput()?[location];
        if requested > limit {
            return Err(SimError::ThroughputExceeded {
                requested,
                limit,
                location,
            });
        }

        // All checks passed; apply. Removals first so a swap is simultaneous.
        self.pending[i] = tally;
        let planned = &mut self.planned;
        if let Some(d) = unload {
            planned.vessels[i].cargo.remove_one(d);
        }
        if let Some(d) = load {
            planned.docks[location].remove_one(d);
            planned.vessels[i].cargo.add(d, 1)?;
        }
        if let Some(d) = unload {
            if d == location {
                planned.delivered += 1; // delivered: leaves the simulation
            } else {
                planned.docks[location].add(d, 1)?;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------- history --

    fn state_at(&self, tick: i64) -> SimResult<String> {
        if tick < 0 {
            return Err(SimError::NegativeTick(tick));
        }
        let current = self.committed.tick;
        if tick as u64 > current {
            return Err(SimError::FutureTick { tick, current });
        }
        Ok(self.history[tick as usize].to_json_string(self.cargo_enabled()))
    }
}

// ---------------------------------------------------------------------------
// Validation helpers (pure functions, no state).
// ---------------------------------------------------------------------------

fn check_location(value: i64, field: &str, locations: usize) -> SimResult<usize> {
    usize::try_from(value)
        .ok()
        .filter(|&v| v < locations)
        .ok_or_else(|| SimError::LocationOutOfRange {
            field: field.to_string(),
            value,
            locations,
        })
}

fn validate_travel_times(raw: &[Vec<i64>]) -> SimResult<Vec<Vec<u64>>> {
    let n = raw.len();
    if n == 0 {
        return Err(SimError::EmptyWorld);
    }
    let mut out = Vec::with_capacity(n);
    for (i, row) in raw.iter().enumerate() {
        if row.len() != n {
            return Err(SimError::MatrixNotSquare {
                row: i,
                len: row.len(),
                expected: n,
            });
        }
        for (j, &t) in row.iter().enumerate() {
            if i == j && t != 0 {
                return Err(SimError::NonZeroDiagonal { index: i, value: t });
            }
            if i != j && t <= 0 {
                return Err(SimError::NonPositiveTravelTime {
                    from: i,
                    to: j,
                    value: t,
                });
            }
        }
        out.push(row.iter().map(|&t| t as u64).collect());
    }
    Ok(out)
}

fn validate_throughput(raw: &[i64], locations: usize) -> SimResult<Vec<u64>> {
    if raw.len() != locations {
        return Err(SimError::ThroughputLength {
            len: raw.len(),
            expected: locations,
        });
    }
    raw.iter()
        .enumerate()
        .map(|(i, &t)| {
            if t > 0 {
                Ok(t as u64)
            } else {
                Err(SimError::NonPositiveThroughput {
                    location: i,
                    value: t,
                })
            }
        })
        .collect()
}

/// Returns `(location, destination, quantity)` triples, all known-valid.
fn validate_manifest(
    entries: &[ManifestEntry],
    locations: usize,
) -> SimResult<Vec<(usize, usize, u64)>> {
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let path = format!("cargo_manifest[{i}]");
            let location = check_location(e.location, &format!("{path}.location"), locations)?;
            let destination =
                check_location(e.destination, &format!("{path}.destination"), locations)?;
            if e.quantity <= 0 {
                return Err(SimError::NonPositiveQuantity {
                    field: format!("{path}.quantity"),
                    value: e.quantity,
                });
            }
            if location == destination {
                return Err(SimError::ManifestAlreadyAtDestination {
                    field: path,
                    location,
                });
            }
            Ok((location, destination, e.quantity as u64))
        })
        .collect()
}

/// Adds validated manifest cargo to `world`'s docks; returns units added.
fn apply_manifest(world: &mut World, entries: &[(usize, usize, u64)]) -> SimResult<u64> {
    let mut added: u64 = 0;
    for &(location, destination, quantity) in entries {
        world.docks[location].add(destination, quantity)?;
        added = added
            .checked_add(quantity)
            .ok_or(SimError::Overflow("cargo total"))?;
    }
    Ok(added)
}

// ---------------------------------------------------------------------------
// Public façade.
// ---------------------------------------------------------------------------

/// The whole simulator: feed it lines, get response lines back.
#[derive(Default)]
pub struct Simulator {
    engine: Option<Engine>,
}

impl Simulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process one input line and return the one response line.
    pub fn process(&mut self, line: &str) -> String {
        match self.execute(line) {
            Ok(payload) => ok_response(&payload),
            Err(e) => error_response(&e),
        }
    }

    /// Process one input line, returning the serialized payload or the typed
    /// error.
    pub fn execute(&mut self, line: &str) -> SimResult<String> {
        let envelope = parse_envelope(line)?;
        match (&mut self.engine, envelope.kind) {
            (Some(_), CommandKind::Init) => Err(SimError::AlreadyInitialized),
            (None, CommandKind::Init) => {
                let Command::Init(params) = parse_command(&envelope)? else {
                    unreachable!("init envelope always parses to Command::Init")
                };
                let engine = Engine::init(params)?;
                let payload = engine.planned_json();
                self.engine = Some(engine);
                Ok(payload)
            }
            (None, _) => Err(SimError::NotInitialized),
            (Some(engine), _) => engine.apply(parse_command(&envelope)?),
        }
    }

    // Read-only views, used by tests to check invariants.

    pub fn committed(&self) -> Option<&World> {
        self.engine.as_ref().map(|e| &e.committed)
    }

    pub fn planned(&self) -> Option<&World> {
        self.engine.as_ref().map(|e| &e.planned)
    }

    pub fn history(&self) -> Option<&[World]> {
        self.engine.as_ref().map(|e| e.history.as_slice())
    }

    pub fn total_manifested(&self) -> Option<u64> {
        self.engine.as_ref().map(|e| e.manifested)
    }

    pub fn dock_throughput(&self) -> Option<&[u64]> {
        self.engine
            .as_ref()
            .and_then(|e| e.dock_throughput.as_deref())
    }
}
