use bevy_ecs::prelude::*;
use bevy_math::IVec2;

use crate::world::map::{GridPosition, LevelId};

#[derive(Component)]
pub struct Actor;

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct Monster;

#[derive(Component)]
pub struct BlocksMovement;

#[derive(Component)]
pub struct BlocksSight;

#[derive(Component, Debug, Clone, Copy)]
pub struct Health {
    pub current: i32,
    pub maximum: i32,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct CombatStats {
    pub power: i32,
    pub defense: i32,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct Vision {
    pub range: u32,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct ActionSpeed {
    pub ticks_per_action: u64,
}

#[derive(Component, Debug, Clone)]
pub struct PrototypeId(pub String);

#[derive(Component)]
pub struct HostileToPlayer;

#[derive(Component, Debug, Clone, Copy)]
pub struct LastKnownPlayerPosition {
    pub level: LevelId,
    pub cell: IVec2,
    pub observed_at: u64,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PersistentId(pub u64);

#[derive(Component, Debug, Clone, Default)]
pub enum AiGoal {
    #[default]
    Idle,
    Wander,
    Investigate(GridPosition),
    Chase(Entity),
    Flee(Entity),
}
