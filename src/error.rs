//! Every way a command can fail, in one place.
//!
//! This enum is the behavioral contract of the simulator: if a rule can reject
//! a command, it has a variant here, and every variant has a test in
//! `tests/errors.rs`. Handlers return `Result<_, SimError>` and validate fully
//! before mutating, so an `Err` always means "nothing changed".

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimError {
    // ---- Protocol: the line itself is unusable -------------------------------
    InvalidUtf8,
    MalformedJson(String),
    NotAnObject,
    MissingCommand,
    UnknownCommand(String),

    // ---- Lifecycle -----------------------------------------------------------
    NotInitialized,
    AlreadyInitialized,

    // ---- Parameter shape -----------------------------------------------------
    MissingParam(String),
    InvalidParam {
        name: String,
        expected: &'static str,
    },

    // ---- World definition (init) ---------------------------------------------
    EmptyWorld,
    MatrixNotSquare {
        row: usize,
        len: usize,
        expected: usize,
    },
    NonZeroDiagonal {
        index: usize,
        value: i64,
    },
    NonPositiveTravelTime {
        from: usize,
        to: usize,
        value: i64,
    },
    ThroughputLength {
        len: usize,
        expected: usize,
    },
    NonPositiveThroughput {
        location: usize,
        value: i64,
    },
    EmptyVesselId {
        index: usize,
    },
    DuplicateVessel(String),

    // ---- References into the world -------------------------------------------
    UnknownVessel(String),
    LocationOutOfRange {
        field: String,
        value: i64,
        locations: usize,
    },
    NonPositiveQuantity {
        field: String,
        value: i64,
    },
    ManifestAlreadyAtDestination {
        field: String,
        location: usize,
    },

    // ---- Vessel state machine ------------------------------------------------
    NotIdle(String),
    NotDocked(String),
    AlreadyAtLocation {
        vessel: String,
        location: usize,
    },

    // ---- Cargo ---------------------------------------------------------------
    CargoDisabled,
    NoDockInventory {
        location: usize,
        destination: usize,
    },
    NoVesselCargo {
        vessel: String,
        destination: usize,
    },
    SwapSameCargo {
        vessel: String,
        destination: usize,
    },
    ThroughputExceeded {
        requested: u64,
        limit: u64,
        location: usize,
    },

    // ---- History -------------------------------------------------------------
    NegativeTick(i64),
    FutureTick {
        tick: i64,
        current: u64,
    },

    // ---- Numeric limits ------------------------------------------------------
    Overflow(&'static str),
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use SimError::*;
        match self {
            InvalidUtf8 => write!(f, "input line is not valid UTF-8"),
            MalformedJson(e) => write!(f, "invalid JSON: {e}"),
            NotAnObject => write!(f, "command must be a JSON object"),
            MissingCommand => write!(f, "missing or non-string 'command' field"),
            UnknownCommand(c) => write!(f, "unknown command '{c}'"),

            NotInitialized => write!(
                f,
                "simulation is not initialized: the first command must be init"
            ),
            AlreadyInitialized => write!(f, "simulation is already initialized"),

            MissingParam(p) => write!(f, "missing parameter '{p}'"),
            InvalidParam { name, expected } => write!(f, "parameter '{name}' must be {expected}"),

            EmptyWorld => write!(f, "travel_times must contain at least one location"),
            MatrixNotSquare { row, len, expected } => write!(
                f,
                "travel_times row {row} has {len} entries, expected {expected}"
            ),
            NonZeroDiagonal { index, value } => {
                write!(f, "travel_times[{index}][{index}] must be 0 (got {value})")
            }
            NonPositiveTravelTime { from, to, value } => write!(
                f,
                "travel_times[{from}][{to}] must be a positive integer (got {value})"
            ),
            ThroughputLength { len, expected } => write!(
                f,
                "dock_throughput has {len} entries, expected {expected} (one per location)"
            ),
            NonPositiveThroughput { location, value } => write!(
                f,
                "dock_throughput[{location}] must be a positive integer (got {value})"
            ),
            EmptyVesselId { index } => write!(f, "vessels[{index}].id must be a non-empty string"),
            DuplicateVessel(id) => write!(f, "duplicate vessel id {id}"),

            UnknownVessel(id) => write!(f, "unknown vessel {id}"),
            LocationOutOfRange {
                field,
                value,
                locations,
            } => write!(
                f,
                "{field} {value} is out of range (valid locations are 0 to {})",
                locations - 1
            ),
            NonPositiveQuantity { field, value } => {
                write!(f, "{field} must be a positive integer (got {value})")
            }
            ManifestAlreadyAtDestination { field, location } => write!(
                f,
                "{field}: cargo bound for location {location} cannot originate there"
            ),

            NotIdle(v) => write!(f, "vessel {v} is not idle"),
            NotDocked(v) => write!(f, "vessel {v} is not docked"),
            AlreadyAtLocation { vessel, location } => {
                write!(f, "vessel {vessel} is already at location {location}")
            }

            CargoDisabled => write!(
                f,
                "cargo operations are disabled: init did not provide dock_throughput"
            ),
            NoDockInventory {
                location,
                destination,
            } => write!(
                f,
                "dock at location {location} has no cargo for destination {destination}"
            ),
            NoVesselCargo {
                vessel,
                destination,
            } => write!(
                f,
                "vessel {vessel} has no cargo for destination {destination}"
            ),
            SwapSameCargo {
                vessel,
                destination,
            } => write!(
                f,
                "swap for vessel {vessel} must exchange different cargo (both sides are destination {destination})"
            ),
            ThroughputExceeded {
                requested,
                limit,
                location,
            } => write!(
                f,
                "transfer of {requested} operations exceeds dock throughput of {limit} at location {location}"
            ),

            NegativeTick(t) => write!(f, "tick {t} is negative"),
            FutureTick { tick, current } => {
                write!(f, "tick {tick} is in the future (current tick: {current})")
            }

            Overflow(what) => write!(f, "{what} overflowed"),
        }
    }
}

impl std::error::Error for SimError {}

pub type SimResult<T> = Result<T, SimError>;
