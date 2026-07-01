use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;

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
use sim_core::{ActorId, Cadence, DeterministicDriver, DueWork, FrameAction, SimulationWorkBudget};
use std::cmp::Reverse;

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

#[derive(Resource, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationDriverState {
    pub driver: DeterministicDriver<ActorId>,
}

impl Default for SimulationDriverState {
    fn default() -> Self {
        Self {
            driver: DeterministicDriver::default(),
        }
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
    let continuing = world
        .resource::<SimulationDriverState>()
        .driver
        .pending_target_minute()
        .is_some();
    if *world.resource::<SimulationStatus>() != SimulationStatus::Resolving && !continuing {
        return;
    }
    if world
        .resource::<SimulationDriverState>()
        .driver
        .clock
        .is_paused()
    {
        return;
    }

    let budget = *world.resource::<SimulationWorkBudget>();
    let current_tick = world.resource::<TurnClock>().current_tick;
    let current_actor = world.resource::<CurrentActor>().0;
    let action_queue_actor = world
        .resource::<ActionQueue>()
        .actions
        .front()
        .map(|action| action.actor);
    let tactical_entries =
        tactical_backlog_from_clock(world.resource::<TurnClock>(), current_actor);
    let mut driver_state = world.resource::<SimulationDriverState>().clone();
    driver_state.driver.clock.minute = current_tick;
    driver_state.driver.budget = budget;
    driver_state.driver.begin_frame();
    let mut target = driver_state
        .driver
        .pending_target_minute()
        .unwrap_or_else(|| {
            tactical_entries
                .iter()
                .map(|work| work.due_minute)
                .min()
                .unwrap_or(current_tick)
        });
    let target_actor = current_actor.or(action_queue_actor);
    if let Some(actor) = target_actor {
        if let Some(next_due) = tactical_entries
            .iter()
            .find(|work| work.id == actor)
            .map(|work| work.due_minute)
        {
            target = target.max(next_due);
        }
    }
    driver_state.driver.set_pending_target_minute(Some(target));

    let non_tactical_backlog = driver_state
        .driver
        .backlog
        .entries()
        .into_iter()
        .filter(|work| work.cadence != Cadence::Tactical)
        .collect::<Vec<_>>();

    let mut tactical_driver = driver_state.driver.clone();
    tactical_driver.clear_backlog();
    tactical_driver.replace_backlog(tactical_entries.iter().copied());

    loop {
        let stopped = tactical_driver
            .run_frame_controlled(|_, work| match work.cadence {
                Cadence::Tactical => {
                    if let Some(mut clock) = world.get_resource_mut::<TurnClock>() {
                        clock.current_tick = work.due_minute;
                    }
                    world.run_schedule(SimulationStep);
                    match *world.resource::<SimulationStatus>() {
                        SimulationStatus::WaitingForPlayer => FrameAction::Yield(1),
                        SimulationStatus::GameOver => FrameAction::Terminal(1),
                        SimulationStatus::Resolving => FrameAction::Continue(1),
                    }
                }
                _ => FrameAction::Continue(work.domain_event_cost),
            })
            .expect("simulation driver should not exceed its configured budget");

        if !stopped || *world.resource::<SimulationStatus>() != SimulationStatus::Resolving {
            break;
        }

        if tactical_driver.pending_target_minute().is_none() {
            break;
        }

        let tactical_remaining = tactical_backlog_from_clock(
            world.resource::<TurnClock>(),
            world.resource::<CurrentActor>().0,
        );
        let mut rebuilt_backlog = non_tactical_backlog.clone();
        rebuilt_backlog.extend(tactical_remaining.iter().copied());
        tactical_driver.clear_backlog();
        tactical_driver.replace_backlog(rebuilt_backlog);
    }

    let tactical_remaining = tactical_backlog_from_clock(
        world.resource::<TurnClock>(),
        world.resource::<CurrentActor>().0,
    );
    let mut merged_backlog = non_tactical_backlog;
    merged_backlog.extend(tactical_remaining.iter().copied());

    let pending_target = tactical_driver.pending_target_minute();
    driver_state.driver = tactical_driver;
    driver_state.driver.backlog.clear();
    driver_state.driver.replace_backlog(merged_backlog);
    driver_state
        .driver
        .set_pending_target_minute(pending_target);
    let current_actor_after = world.resource::<CurrentActor>().0;
    let action_queue_actor_after = world
        .resource::<ActionQueue>()
        .actions
        .front()
        .map(|action| action.actor);
    if pending_target.is_none()
        && current_actor_after.is_none()
        && action_queue_actor_after.is_none()
        && *world.resource::<SimulationStatus>() == SimulationStatus::Resolving
    {
        if let Some(mut status) = world.get_resource_mut::<SimulationStatus>() {
            *status = SimulationStatus::WaitingForPlayer;
        }
    }
    world.insert_resource(driver_state.clone());

    if let Some(mut clock) = world.get_resource_mut::<TurnClock>() {
        clock.current_tick = driver_state.driver.clock.minute;
    }
}

fn tactical_backlog_from_clock(
    clock: &TurnClock,
    current_actor: Option<ActorId>,
) -> Vec<DueWork<ActorId>> {
    let mut backlog = Vec::with_capacity(clock.timeline.len());
    for entry in clock.timeline.iter() {
        backlog.push(DueWork {
            cadence: Cadence::Tactical,
            due_minute: entry.0.next_tick,
            sequence: entry.0.sequence,
            id: entry.0.actor,
            domain_event_cost: 1,
        });
    }
    if let Some(actor) = current_actor
        && backlog.iter().all(|entry| entry.id != actor)
    {
        backlog.push(DueWork {
            cadence: Cadence::Tactical,
            due_minute: clock.current_tick,
            sequence: 0,
            id: actor,
            domain_event_cost: 1,
        });
    }
    backlog
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
