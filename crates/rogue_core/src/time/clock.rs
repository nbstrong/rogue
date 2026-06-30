use crate::actor::components::ActorId;

pub type CurrentActor = sim_core::schedule::CurrentActor<ActorId>;
pub type ScheduledActor = sim_core::schedule::ScheduledWork<ActorId>;
pub type TurnClock = sim_core::schedule::TurnClock<ActorId>;
