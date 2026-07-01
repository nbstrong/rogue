use std::collections::{HashMap, VecDeque};

use bevy_ecs::prelude::*;

use crate::actor::combat::{DamageKind, StatusEffect};
use crate::actor::components::{ActiveStatuses, ActorId, Health, StableEntityIndex};
use crate::world::map::GridPosition;

#[derive(Debug, Clone)]
pub enum Effect {
    Damage {
        source: Option<ActorId>,
        target: ActorId,
        amount: i32,
        kind: DamageKind,
    },
    Heal {
        target: ActorId,
        amount: i32,
    },
    Teleport {
        target: ActorId,
        destination: GridPosition,
    },
    ApplyStatus {
        target: ActorId,
        status: StatusEffect,
    },
}

#[derive(Resource, Default, Debug, Clone)]
pub struct EffectQueue(pub VecDeque<Effect>);

pub fn apply_pending_effects(
    mut effects: ResMut<'_, EffectQueue>,
    stable_index: Res<'_, StableEntityIndex>,
    mut health: Query<'_, '_, &mut Health>,
    mut statuses: Query<'_, '_, &mut ActiveStatuses>,
    mut commands: Commands<'_, '_>,
) {
    let mut pending_status_inserts: HashMap<ActorId, Vec<StatusEffect>> = HashMap::new();

    while let Some(effect) = effects.0.pop_front() {
        match effect {
            Effect::Damage { target, amount, .. } => {
                if let Some(entity) = stable_index.actor(target)
                    && let Ok(mut hp) = health.get_mut(entity)
                {
                    hp.current -= amount;
                }
            }
            Effect::Heal { target, amount } => {
                if let Some(entity) = stable_index.actor(target)
                    && let Ok(mut hp) = health.get_mut(entity)
                {
                    hp.current = (hp.current + amount).min(hp.maximum);
                }
            }
            Effect::Teleport {
                target,
                destination,
            } => {
                if let Some(entity) = stable_index.actor(target) {
                    commands.entity(entity).insert(destination);
                }
            }
            Effect::ApplyStatus { target, status } => {
                if let Some(entity) = stable_index.actor(target) {
                    if let Ok(mut active_statuses) = statuses.get_mut(entity) {
                        active_statuses.0.push(status);
                    } else {
                        pending_status_inserts
                            .entry(target)
                            .or_default()
                            .push(status);
                    }
                }
            }
        }
    }

    for (target, statuses) in pending_status_inserts {
        if let Some(entity) = stable_index.actor(target) {
            commands.entity(entity).insert(ActiveStatuses(statuses));
        }
    }
}
