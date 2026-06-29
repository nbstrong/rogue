use std::collections::VecDeque;

use bevy_ecs::prelude::*;

use crate::actor::combat::{DamageKind, StatusEffect};
use crate::actor::components::Health;
use crate::world::map::GridPosition;

#[derive(Debug, Clone)]
pub enum Effect {
    Damage {
        source: Option<Entity>,
        target: Entity,
        amount: i32,
        kind: DamageKind,
    },
    Heal {
        target: Entity,
        amount: i32,
    },
    Teleport {
        target: Entity,
        destination: GridPosition,
    },
    ApplyStatus {
        target: Entity,
        status: StatusEffect,
    },
}

#[derive(Resource, Default, Debug, Clone)]
pub struct EffectQueue(pub VecDeque<Effect>);

pub fn apply_pending_effects(
    mut effects: ResMut<'_, EffectQueue>,
    mut health: Query<'_, '_, &mut Health>,
    mut commands: Commands<'_, '_>,
) {
    while let Some(effect) = effects.0.pop_front() {
        match effect {
            Effect::Damage { target, amount, .. } => {
                if let Ok(mut hp) = health.get_mut(target) {
                    hp.current -= amount;
                }
            }
            Effect::Heal { target, amount } => {
                if let Ok(mut hp) = health.get_mut(target) {
                    hp.current = (hp.current + amount).min(hp.maximum);
                }
            }
            Effect::Teleport {
                target,
                destination,
            } => {
                commands.entity(target).insert(destination);
            }
            Effect::ApplyStatus { .. } => {}
        }
    }
}
