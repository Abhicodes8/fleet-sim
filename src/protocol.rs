//! Wire format: one JSON object per line in, one per line out.
//!
//! Parsing happens in two stages so errors come out in a sensible order:
//!
//! 1. [`parse_envelope`] checks the line is JSON, is an object, and names a
//!    known command. The engine then checks lifecycle (init first, init once).
//! 2. [`parse_command`] checks the parameter *shapes* (types, required fields)
//!    and produces a typed [`Command`]. Range checks that need the world
//!    (location indices, vessel ids) happen later, in the engine.
//!
//! Parameters are extracted by hand from `serde_json::Value` rather than via
//! derive so that every error names the exact offending field, e.g.
//! `parameter 'cargo_manifest[2].quantity' must be an integer`.

use serde_json::{Map, Value};

use crate::error::{SimError, SimResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Init,
    Tick,
    SetDestination,
    Dock,
    Undock,
    Load,
    Unload,
    Swap,
    GetState,
    GetStateAt,
}

impl CommandKind {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "init" => Self::Init,
            "tick" => Self::Tick,
            "set_destination" => Self::SetDestination,
            "dock" => Self::Dock,
            "undock" => Self::Undock,
            "load" => Self::Load,
            "unload" => Self::Unload,
            "swap" => Self::Swap,
            "get_state" => Self::GetState,
            "get_state_at" => Self::GetStateAt,
            _ => return None,
        })
    }
}

#[derive(Debug)]
pub struct Envelope {
    pub kind: CommandKind,
    pub params: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VesselSpec {
    pub id: String,
    pub location: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub location: i64,
    pub destination: i64,
    pub quantity: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitParams {
    pub travel_times: Vec<Vec<i64>>,
    pub vessels: Vec<VesselSpec>,
    pub dock_throughput: Option<Vec<i64>>,
    pub cargo_manifest: Option<Vec<ManifestEntry>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Init(InitParams),
    Tick {
        cargo_manifest: Option<Vec<ManifestEntry>>,
    },
    SetDestination {
        vessel_id: String,
        destination: i64,
    },
    Dock {
        vessel_id: String,
    },
    Undock {
        vessel_id: String,
    },
    Load {
        vessel_id: String,
        destination: i64,
    },
    Unload {
        vessel_id: String,
        destination: i64,
    },
    Swap {
        vessel_id: String,
        load_destination: i64,
        unload_destination: i64,
    },
    GetState,
    GetStateAt {
        tick: i64,
    },
}

/// Stage 1: is this a well-formed request for a known command?
pub fn parse_envelope(line: &str) -> SimResult<Envelope> {
    let value: Value =
        serde_json::from_str(line).map_err(|e| SimError::MalformedJson(e.to_string()))?;
    let Value::Object(mut obj) = value else {
        return Err(SimError::NotAnObject);
    };
    let name = match obj.get("command") {
        Some(Value::String(s)) => s.clone(),
        _ => return Err(SimError::MissingCommand),
    };
    let kind = CommandKind::from_name(&name).ok_or(SimError::UnknownCommand(name))?;
    // A missing or null `parameters` is treated as `{}` so `{"command":"tick"}` works.
    let params = match obj.remove("parameters") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m,
        Some(_) => return Err(invalid("parameters", "an object")),
    };
    Ok(Envelope { kind, params })
}

/// Stage 2: are the parameters the right shape for this command?
pub fn parse_command(env: &Envelope) -> SimResult<Command> {
    let p = &env.params;
    Ok(match env.kind {
        CommandKind::Init => Command::Init(InitParams {
            travel_times: travel_times(required(p, "travel_times")?)?,
            vessels: vessels(required(p, "vessels")?)?,
            dock_throughput: optional(p, "dock_throughput")
                .map(|v| int_array(v, "dock_throughput"))
                .transpose()?,
            cargo_manifest: optional(p, "cargo_manifest").map(manifest).transpose()?,
        }),
        CommandKind::Tick => Command::Tick {
            cargo_manifest: optional(p, "cargo_manifest").map(manifest).transpose()?,
        },
        CommandKind::SetDestination => Command::SetDestination {
            vessel_id: vessel_id(p)?,
            destination: int_param(p, "destination")?,
        },
        CommandKind::Dock => Command::Dock {
            vessel_id: vessel_id(p)?,
        },
        CommandKind::Undock => Command::Undock {
            vessel_id: vessel_id(p)?,
        },
        CommandKind::Load => Command::Load {
            vessel_id: vessel_id(p)?,
            destination: int_param(p, "destination")?,
        },
        CommandKind::Unload => Command::Unload {
            vessel_id: vessel_id(p)?,
            destination: int_param(p, "destination")?,
        },
        CommandKind::Swap => Command::Swap {
            vessel_id: vessel_id(p)?,
            load_destination: int_param(p, "load_destination")?,
            unload_destination: int_param(p, "unload_destination")?,
        },
        CommandKind::GetState => Command::GetState,
        CommandKind::GetStateAt => Command::GetStateAt {
            tick: int_param(p, "tick")?,
        },
    })
}

/// Build the success response line around an already-serialized payload.
pub fn ok_response(payload: &str) -> String {
    format!(r#"{{"status":"ok","payload":{payload}}}"#)
}

/// Build the error response line.
pub fn error_response(err: &SimError) -> String {
    let message = Value::from(err.to_string()); // escapes the text as JSON
    format!(r#"{{"status":"error","payload":{{"message":{message}}}}}"#)
}

// ---------------------------------------------------------------------------
// Field extraction helpers. Each takes the field's full path for error text.
// ---------------------------------------------------------------------------

fn invalid(name: impl Into<String>, expected: &'static str) -> SimError {
    SimError::InvalidParam {
        name: name.into(),
        expected,
    }
}

/// Missing and `null` are both "not provided".
fn optional<'a>(map: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    map.get(name).filter(|v| !v.is_null())
}

fn required<'a>(map: &'a Map<String, Value>, name: &str) -> SimResult<&'a Value> {
    optional(map, name).ok_or_else(|| SimError::MissingParam(name.to_string()))
}

/// Integers only: `1.5`, `1.0`, `"1"`, and values outside i64 are all rejected.
fn as_int(value: &Value, path: &str) -> SimResult<i64> {
    value.as_i64().ok_or_else(|| invalid(path, "an integer"))
}

fn as_array<'a>(value: &'a Value, path: &str) -> SimResult<&'a Vec<Value>> {
    value.as_array().ok_or_else(|| invalid(path, "an array"))
}

fn as_object<'a>(value: &'a Value, path: &str) -> SimResult<&'a Map<String, Value>> {
    value.as_object().ok_or_else(|| invalid(path, "an object"))
}

fn int_param(map: &Map<String, Value>, name: &str) -> SimResult<i64> {
    as_int(required(map, name)?, name)
}

fn vessel_id(map: &Map<String, Value>) -> SimResult<String> {
    required(map, "vessel_id")?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("vessel_id", "a string"))
}

fn int_array(value: &Value, path: &str) -> SimResult<Vec<i64>> {
    as_array(value, path)?
        .iter()
        .enumerate()
        .map(|(i, v)| as_int(v, &format!("{path}[{i}]")))
        .collect()
}

fn travel_times(value: &Value) -> SimResult<Vec<Vec<i64>>> {
    as_array(value, "travel_times")?
        .iter()
        .enumerate()
        .map(|(i, row)| int_array(row, &format!("travel_times[{i}]")))
        .collect()
}

fn vessels(value: &Value) -> SimResult<Vec<VesselSpec>> {
    as_array(value, "vessels")?
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let path = format!("vessels[{i}]");
            let obj = as_object(v, &path)?;
            let id = optional(obj, "id")
                .ok_or_else(|| SimError::MissingParam(format!("{path}.id")))?
                .as_str()
                .ok_or_else(|| invalid(format!("{path}.id"), "a string"))?
                .to_owned();
            let location = as_int(
                optional(obj, "location")
                    .ok_or_else(|| SimError::MissingParam(format!("{path}.location")))?,
                &format!("{path}.location"),
            )?;
            Ok(VesselSpec { id, location })
        })
        .collect()
}

fn manifest(value: &Value) -> SimResult<Vec<ManifestEntry>> {
    as_array(value, "cargo_manifest")?
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let path = format!("cargo_manifest[{i}]");
            let obj = as_object(entry, &path)?;
            let field = |name: &str| -> SimResult<i64> {
                let full = format!("{path}.{name}");
                as_int(
                    optional(obj, name).ok_or_else(|| SimError::MissingParam(full.clone()))?,
                    &full,
                )
            };
            Ok(ManifestEntry {
                location: field("location")?,
                destination: field("destination")?,
                quantity: field("quantity")?,
            })
        })
        .collect()
}
