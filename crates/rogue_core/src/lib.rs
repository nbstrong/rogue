#![forbid(unsafe_code)]

pub mod action;
pub mod actor;
pub mod content;
pub mod item;
pub mod persistence;
pub mod simulation;
pub mod time;
pub mod world;

pub use action::intent::{Action, ActionKind, ActionTarget};
pub use actor::components::*;
pub use simulation::{
    SimulationPlugin, SimulationSet, SimulationStatus, SimulationStep, drive_simulation,
};
pub use time::clock::{ScheduledActor, TurnClock};
pub use world::map::{GridPosition, LevelId};
