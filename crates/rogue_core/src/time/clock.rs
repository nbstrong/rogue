use bevy_ecs::prelude::Entity;

pub type CurrentActor = sim_core::schedule::CurrentActor<Entity>;
pub type ScheduledActor = sim_core::schedule::ScheduledWork<Entity>;
pub type TurnClock = sim_core::schedule::TurnClock<Entity>;
