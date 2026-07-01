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
use sim_core::{ActorId, Cadence, DeterministicDriver, DueWork, SimulationWorkBudget};
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
    let target = {
        let mut driver_state = world.resource_mut::<SimulationDriverState>();
        driver_state.driver.clock.minute = current_tick;
        driver_state.driver.budget = budget;
        driver_state.driver.begin_frame();
        driver_state
            .driver
            .pending_target_minute()
            .unwrap_or_else(|| driver_state.driver.target_minute())
    };
    sync_tactical_backlog(world);

    let mut processed_steps = 0usize;
    for _ in 0..budget.maximum_steps_per_frame {
        if *world.resource::<SimulationStatus>() != SimulationStatus::Resolving {
            break;
        }
        let current_tick = world.resource::<TurnClock>().current_tick;
        if current_tick > target {
            break;
        }
        if world
            .resource::<TurnClock>()
            .peek_next()
            .is_some_and(|entry| entry.next_tick > target)
        {
            break;
        }

        let outcomes_before = world.resource::<ActionOutcomeLog>().outcomes.len();
        world.run_schedule(SimulationStep);
        let outcomes_after = world.resource::<ActionOutcomeLog>().outcomes.len();
        let produced = outcomes_after.saturating_sub(outcomes_before).max(1);
        processed_steps = processed_steps.saturating_add(1);

        let current_tick = world.resource::<TurnClock>().current_tick;
        let mut driver_state = world.resource_mut::<SimulationDriverState>();
        driver_state.driver.progress.consume_step();
        driver_state.driver.progress.consume_domain_events(produced);
        driver_state.driver.clock.minute = current_tick;
        drop(driver_state);
        sync_tactical_backlog(world);

        if *world.resource::<SimulationStatus>() != SimulationStatus::Resolving {
            break;
        }
    }

    let still_resolving = *world.resource::<SimulationStatus>() == SimulationStatus::Resolving;
    let pending_target = if still_resolving { Some(target) } else { None };

    let current_tick = world.resource::<TurnClock>().current_tick;
    let tactical_entries = {
        let clock = world.resource::<TurnClock>();
        clock
            .timeline
            .iter()
            .map(|entry| DueWork {
                cadence: Cadence::Tactical,
                due_minute: entry.0.next_tick,
                sequence: entry.0.sequence,
                id: entry.0.actor,
                domain_event_cost: 1,
            })
            .collect::<Vec<_>>()
    };
    let mut driver_state = world.resource_mut::<SimulationDriverState>();
    driver_state.driver.clock.minute = current_tick;
    sync_tactical_backlog_in_driver(&mut driver_state.driver, tactical_entries);
    driver_state
        .driver
        .set_pending_target_minute(pending_target);
}

fn sync_tactical_backlog(world: &mut World) {
    let tactical_entries = {
        let clock = world.resource::<TurnClock>();
        clock
            .timeline
            .iter()
            .map(|entry| DueWork {
                cadence: Cadence::Tactical,
                due_minute: entry.0.next_tick,
                sequence: entry.0.sequence,
                id: entry.0.actor,
                domain_event_cost: 1,
            })
            .collect::<Vec<_>>()
    };
    let mut driver_state = world.resource_mut::<SimulationDriverState>();
    sync_tactical_backlog_in_driver(&mut driver_state.driver, tactical_entries);
}

fn sync_tactical_backlog_in_driver(
    driver: &mut DeterministicDriver<ActorId>,
    tactical_entries: Vec<DueWork<ActorId>>,
) {
    driver
        .backlog
        .retain_where(|work| work.cadence != Cadence::Tactical);
    for entry in tactical_entries {
        driver.enqueue(entry);
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
