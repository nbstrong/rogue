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
use sim_core::{
    Cadence, DeterministicDriver, DomainWorkId, DueWork, FrameAction, SimulationWorkBudget,
};

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
    #[serde(default)]
    target_minute: Option<u64>,
}

impl DomainAdvanceRequest {
    pub fn target_minute(&self) -> Option<u64> {
        self.target_minute
    }

    fn set_target_minute(&mut self, target_minute: Option<u64>) {
        self.target_minute = target_minute;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainWorkError {
    TacticalCadence {
        id: DomainWorkId,
    },
    InvalidEventCost {
        id: DomainWorkId,
        declared_cost: usize,
    },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainAdvanceError {
    TargetPrecedesClock {
        clock_minute: u64,
        requested_minute: u64,
    },
    RequestAlreadyActive {
        active_target: u64,
        requested_minute: u64,
    },
}

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
    pub fn request_advance(&mut self, target_minute: u64) -> Result<(), DomainAdvanceError> {
        if target_minute < self.driver.clock.minute {
            return Err(DomainAdvanceError::TargetPrecedesClock {
                clock_minute: self.driver.clock.minute,
                requested_minute: target_minute,
            });
        }

        match self.request.target_minute() {
            Some(active_target) if active_target != target_minute => {
                Err(DomainAdvanceError::RequestAlreadyActive {
                    active_target,
                    requested_minute: target_minute,
                })
            }
            Some(_) => Ok(()),
            None => {
                self.request.set_target_minute(Some(target_minute));
                self.driver.set_pending_target_minute(None);
                Ok(())
            }
        }
    }

    pub fn enqueue_work(
        &mut self,
        cadence: Cadence,
        due_minute: u64,
        sequence: u64,
        id: DomainWorkId,
    ) -> Result<(), DomainWorkError> {
        if cadence == Cadence::Tactical {
            return Err(DomainWorkError::TacticalCadence { id });
        }
        self.driver.enqueue(DueWork {
            cadence,
            due_minute,
            sequence,
            id,
            domain_event_cost: 1,
        });
        Ok(())
    }

    pub fn clear_request(&mut self) {
        self.request.set_target_minute(None);
        self.driver.set_pending_target_minute(None);
    }

    pub fn has_active_domain_request(&self) -> bool {
        self.request.target_minute().is_some() || self.driver.pending_target_minute().is_some()
    }

    pub fn has_scheduled_domain_work(&self) -> bool {
        !self.driver.backlog.is_empty()
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
        .has_active_domain_request();
    let has_tactical_work = status == SimulationStatus::Resolving;
    if !has_tactical_work && !has_active_domain {
        return;
    }
    let budget = *world.resource::<SimulationWorkBudget>();
    let mut remaining_steps = budget.maximum_steps_per_frame;
    if has_tactical_work {
        while remaining_steps > 0
            && *world.resource::<SimulationStatus>() == SimulationStatus::Resolving
        {
            world.run_schedule(SimulationStep);
            remaining_steps -= 1;
        }

        if remaining_steps == 0 {
            return;
        }
    }

    if remaining_steps == 0 {
        return;
    }

    let invalid_domain_work = {
        let state = world.resource::<SimulationDriverState>();
        state
            .driver
            .backlog
            .entries()
            .into_iter()
            .find(|work| work.cadence == Cadence::Tactical || work.domain_event_cost != 1)
    };
    if let Some(work) = invalid_domain_work {
        panic!(
            "invalid production domain work before execution: {:?}",
            work
        );
    }

    if world
        .resource::<SimulationDriverState>()
        .driver
        .clock
        .is_paused()
    {
        return;
    }

    let (final_target, frame_target) = {
        let state = world.resource::<SimulationDriverState>();
        let final_target = state
            .request
            .target_minute()
            .or_else(|| state.driver.pending_target_minute());
        let Some(final_target) = final_target else {
            return;
        };

        let frame_target = state
            .driver
            .pending_target_minute()
            .or_else(|| {
                let window = state.driver.clock.speed.advance_minutes();
                if window == 0 {
                    None
                } else {
                    Some(final_target.min(state.driver.clock.minute.saturating_add(window)))
                }
            })
            .unwrap_or(final_target);

        (final_target, frame_target)
    };

    let mut domain_state = world.resource_mut::<SimulationDriverState>();
    domain_state.driver.budget = SimulationWorkBudget {
        maximum_steps_per_frame: remaining_steps,
        maximum_domain_events_per_frame: budget.maximum_domain_events_per_frame,
    };
    domain_state.driver.begin_frame();
    domain_state
        .driver
        .set_pending_target_minute(Some(frame_target));
    let mut event_log = Vec::new();
    std::mem::swap(&mut event_log, &mut domain_state.event_log);
    let _ = domain_state
        .driver
        .run_frame_controlled(|_, work| {
            assert_eq!(
                work.domain_event_cost, 1,
                "domain work must emit exactly one event"
            );
            event_log.push(DomainWorkEvent {
                cadence: work.cadence,
                due_minute: work.due_minute,
                sequence: work.sequence,
                id: work.id,
            });
            FrameAction::Continue(1)
        })
        .expect("domain driver should not exceed its configured budget");
    domain_state.event_log = event_log;

    if domain_state.driver.pending_target_minute().is_none()
        && domain_state.driver.clock.minute >= final_target
    {
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
