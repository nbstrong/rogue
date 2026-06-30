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
use crate::time::clock::{CurrentActor, TurnClock};
use crate::time::scheduler::{finish_simulation_step, select_next_actor};
use crate::world::fov::recalculate_fov;
use crate::world::spatial::{SpatialIndex, update_spatial_index};
use sim_core::SimulationWorkBudget;

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
    let max_steps = world
        .get_resource::<SimulationWorkBudget>()
        .map(|budget| budget.maximum_steps_per_frame)
        .unwrap_or(1_024);

    for _ in 0..max_steps {
        let status = *world.resource::<SimulationStatus>();

        if status != SimulationStatus::Resolving {
            return;
        }

        world.run_schedule(SimulationStep);
    }

    return;
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

pub fn rebuild_stable_entity_index(
    mut index: ResMut<'_, StableEntityIndex>,
    actors: Query<'_, '_, (Entity, &StableActorId), With<Actor>>,
    items: Query<'_, '_, (Entity, &StableItemId), With<Item>>,
) {
    index.actors.clear();
    index.items.clear();

    for (entity, stable_id) in actors.iter() {
        index.actors.insert(stable_id.0, entity);
    }

    for (entity, stable_id) in items.iter() {
        index.items.insert(stable_id.0, entity);
    }
}
