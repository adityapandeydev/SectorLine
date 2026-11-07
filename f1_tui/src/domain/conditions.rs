//! Small source-timed side channels, projected independently of core telemetry cadence.
use super::stints::*;
use super::*;
use crate::data::openf1::Result;
use std::collections::{BTreeMap, BTreeSet};
mod archive;
mod project;
pub use archive::parse_control;
#[derive(Debug, Clone, PartialEq)]
pub struct Reading<T> {
    pub at: UtcMillis,
    pub available: Availability,
    pub retrieved: UtcMillis,
    pub value: Option<T>,
}
impl<T> Reading<T> {
    pub fn new(
        at: UtcMillis,
        available: Availability,
        retrieved: UtcMillis,
        value: Option<T>,
    ) -> Result<Self> {
        if matches!(available,Availability::Known(t)|Availability::Estimated(t) if t<at) {
            return Err("Availability precedes condition evidence".into());
        }
        Ok(Self {
            at,
            available,
            retrieved,
            value,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Weather {
    pub air_c: Option<f64>,
    pub track_c: Option<f64>,
    pub humidity: Option<f64>,
    pub pressure_mbar: Option<f64>,
    pub wind_mps: Option<f64>,
    pub wind_degrees: Option<f64>,
    pub rain: Option<bool>,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Scope {
    Track,
    Sector(u32),
    Driver(u32),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Flag {
    Unavailable,
    Green,
    Yellow,
    DoubleYellow,
    Red,
    Chequered,
    Blue,
    Clear,
    Unknown(String),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafetyCar {
    Deployed,
    Ending,
    InThisLap,
    VirtualDeployed,
    VirtualEnding,
    Returned,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    Started,
    Resumed,
    Suspended,
    Finished,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlKind {
    Flag(Flag),
    SafetyCar(SafetyCar),
    Session(SessionStatus),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Control {
    pub scope: Scope,
    pub kind: ControlKind,
}
#[derive(Default)]
pub struct ConditionsArchive {
    pub weather: Vec<Reading<Weather>>,
    pub control: Vec<Reading<Control>>,
    pub stints: BTreeMap<u32, Vec<Reading<Stint>>>,
    pub pits: BTreeMap<u32, Vec<Reading<PitRecord>>>,
    pub completed_laps: BTreeMap<u32, Vec<(u32, UtcMillis)>>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DriverConditions {
    pub tyre: Observation<Tyre>,
    pub pit: Observation<Pit>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConditionsFrame {
    pub air_c: Observation<f64>,
    pub track_c: Observation<f64>,
    pub humidity: Observation<f64>,
    pub pressure_mbar: Observation<f64>,
    pub wind_mps: Observation<f64>,
    pub wind_degrees: Observation<f64>,
    pub rain: Observation<bool>,
    pub track_flag: Observation<Flag>,
    pub local_flags: Vec<(Scope, Observation<Flag>)>,
    pub safety_car: Observation<SafetyCar>,
    pub session_status: Observation<SessionStatus>,
    pub drivers: BTreeMap<u32, DriverConditions>,
}
