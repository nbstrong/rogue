use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;
use std::cmp::Reverse;

use crate::action::queue::ActionQueue;
use crate::action::resolver::{ActionDecision, ActionOutcomeLog, resolve_action, validate_action};
use crate::actor::ai::generate_ai_action;
use crate::actor::components::{
    Actor, PersistentIdAllocator, StableActorId, StableEntityIndex, StableItemId,
};
use crate::item::components::Item;
use crate::item::effects::{EffectQueue, apply_pending_effects};
use crate::persistence::rng::RandomStreams;
use crate::time::clock::ScheduledActor;
use crate::time::clock::{CurrentActor, TurnClock};
use crate::time::scheduler::{finish_simulation_step, select_next_actor};
use crate::world::fov::recalculate_fov;
use crate::world::spatial::{SpatialIndex, update_spatial_index};
use serde::{Deserialize, Serialize};
use sim_core::{Cadence, DeterministicDriver, DomainWorkId, FrameAction, SimulationWorkBudget};

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimulationStep;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimulationSet {
    SelectActor,
    DecideAction,
    Validate,
    Resolve,
    ApplyEffects,
    HandleDeath,
    RebuildDerivedData,
    FinishStep,
}

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationStatus {
    WaitingForPlayer,
    Resolving,
    GameOver,
}

impl Default for SimulationStatus {
    fn default() -> Self {
        Self::WaitingForPlayer
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DomainAdvanceRequest {
    pub target_minute: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainWorkEvent {
    pub cadence: Cadence,
    pub due_minute: u64,
    pub sequence: u64,
    pub id: DomainWorkId,
}

#[derive(Resource, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainSimulationState {
    pub driver: DeterministicDriver<DomainWorkId>,
    #[serde(default)]
    pub request: DomainAdvanceRequest,
    #[serde(default)]
    pub event_log: Vec<DomainWorkEvent>,
}

pub type SimulationDriverState = DomainSimulationState;

impl Default for DomainSimulationState {
    fn default() -> Self {
        Self {
            driver: DeterministicDriver::default(),
            request: DomainAdvanceRequest::default(),
            event_log: Vec::new(),
        }
    }
}

impl DomainSimulationState {
    pub fn request_advance(&mut self, target_minute: u64) {
        self.request.target_minute = Some(target_minute);
        self.driver.set_pending_target_minute(Some(target_minute));
    }

    pub fn clear_request(&mut self) {
        self.request.target_minute = None;
    }

    pub fn has_active_domain_work(&self) -> bool {
        self.request.target_minute.is_some()
            || self.driver.pending_target_minute().is_some()
            || !self.driver.backlog.is_empty()
    }
}

pub struct SimulationPlugin;

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActionQueue>()
            .init_resource::<ActionDecision>()
            .init_resource::<ActionOutcomeLog>()
            .init_resource::<EffectQueue>()
            .init_resource::<TurnClock>()
            .init_resource::<CurrentActor>()
            .init_resource::<SpatialIndex>()
            .init_resource::<StableEntityIndex>()
            .init_resource::<RandomStreams>()
            .init_resource::<PersistentIdAllocator>()
            .init_resource::<SimulationDriverState>()
            .init_resource::<SimulationWorkBudget>()
            .init_resource::<SimulationStatus>()
            .configure_sets(
                SimulationStep,
                (
                    SimulationSet::SelectActor,
                    SimulationSet::DecideAction,
                    SimulationSet::Validate,
                    SimulationSet::Resolve,
                    SimulationSet::ApplyEffects,
                    SimulationSet::HandleDeath,
                    SimulationSet::RebuildDerivedData,
                    SimulationSet::FinishStep,
                )
                    .chain(),
            )
            .add_systems(
                SimulationStep,
                (
                    select_next_actor.in_set(SimulationSet::SelectActor),
                    generate_ai_action.in_set(SimulationSet::DecideAction),
                    validate_action.in_set(SimulationSet::Validate),
                    resolve_action.in_set(SimulationSet::Resolve),
                    apply_pending_effects.in_set(SimulationSet::ApplyEffects),
                    remove_dead_entities.in_set(SimulationSet::HandleDeath),
                    prune_stale_timeline.in_set(SimulationSet::HandleDeath),
                    (
                        rebuild_stable_entity_index,
                        update_spatial_index,
                        recalculate_fov,
                    )
                        .chain()
                        .in_set(SimulationSet::RebuildDerivedData),
                    finish_simulation_step.in_set(SimulationSet::FinishStep),
                ),
            );
    }
}

pub fn drive_simulation(world: &mut World) {
    let status = *world.resource::<SimulationStatus>();
    let has_active_domain = world
        .resource::<SimulationDriverState>()
        .has_active_domain_work();
    let has_tactical_work = status == SimulationStatus::Resolving;
    if !has_tactical_work && !has_active_domain {
        return;
    }
    let budget = *world.resource::<SimulationWorkBudget>();
    if has_tactical_work {
        let mut tactical_steps_remaining = budget.maximum_steps_per_frame;
        while tactical_steps_remaining > 0
            && *world.resource::<SimulationStatus>() == SimulationStatus::Resolving
        {
            world.run_schedule(SimulationStep);
            tactical_steps_remaining -= 1;
        }

        if tactical_steps_remaining == 0 && !has_active_domain {
            return;
        }
    }

    if world
        .resource::<SimulationDriverState>()
        .driver
        .clock
        .is_paused()
    {
        return;
    }

    let target = world
        .resource::<SimulationDriverState>()
        .request
        .target_minute
        .or_else(|| {
            world
                .resource::<SimulationDriverState>()
                .driver
                .pending_target_minute()
        });
    let Some(target) = target else {
        return;
    };

    let mut domain_state = world.resource_mut::<SimulationDriverState>();
    domain_state.driver.budget = budget;
    domain_state.driver.begin_frame();
    domain_state.driver.set_pending_target_minute(Some(target));
    let mut event_log = Vec::new();
    std::mem::swap(&mut event_log, &mut domain_state.event_log);
    let _ = domain_state
        .driver
        .run_frame_controlled(|_, work| {
            event_log.push(DomainWorkEvent {
                cadence: work.cadence,
                due_minute: work.due_minute,
                sequence: work.sequence,
                id: work.id,
            });
            FrameAction::Continue(work.domain_event_cost)
        })
        .expect("domain driver should not exceed its configured budget");
    domain_state.event_log = event_log;

    if domain_state.driver.pending_target_minute().is_none() {
        domain_state.clear_request();
    }
}

pub fn remove_dead_entities(
    mut commands: Commands<'_, '_>,
    query: Query<'_, '_, (Entity, &crate::actor::components::Health)>,
    players: Query<'_, '_, Entity, With<crate::actor::components::Player>>,
    mut status: ResMut<'_, SimulationStatus>,
) {
    for (entity, health) in query.iter() {
        if health.current <= 0 {
            commands.entity(entity).despawn();
        }
    }

    let player_exists = players.iter().any(|entity| {
        query
            .get(entity)
            .is_ok_and(|(_, health)| health.current > 0)
    });
    if !player_exists {
        *status = SimulationStatus::GameOver;
    }
}

pub fn prune_stale_timeline(
    mut clock: ResMut<'_, TurnClock>,
    actors: Query<'_, '_, (&crate::actor::components::Health, &StableActorId), With<Actor>>,
) {
    let mut retained = Vec::new();
    while let Some(entry) = clock.pop_next() {
        if actors
            .iter()
            .any(|(health, stable_id)| health.current > 0 && stable_id.0 == entry.actor)
        {
            retained.push(Reverse(ScheduledActor {
                next_tick: entry.next_tick,
                sequence: entry.sequence,
                actor: entry.actor,
            }));
        }
    }
    clock.timeline = retained.into_iter().collect();
}

pub fn rebuild_stable_entity_index(
    mut index: ResMut<'_, StableEntityIndex>,
    actors: Query<'_, '_, (Entity, &StableActorId), With<Actor>>,
    items: Query<'_, '_, (Entity, &StableItemId), With<Item>>,
) {
    index.clear();

    for (entity, stable_id) in actors.iter() {
        index.insert_actor(stable_id.0, entity);
    }

    for (entity, stable_id) in items.iter() {
        index.insert_item(stable_id.0, entity);
    }
}
