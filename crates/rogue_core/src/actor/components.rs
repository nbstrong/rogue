use bevy_ecs::prelude::*;
use bevy_math::IVec2;
use std::collections::HashMap;

use crate::actor::combat::StatusEffect;
use crate::world::map::{GridPosition, LevelId};

pub type ActorId = sim_core::ActorId;
pub type ItemId = sim_core::ItemId;
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

#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct ActiveStatuses(pub Vec<StatusEffect>);

#[derive(Component, Debug, Clone, Copy)]
pub struct LastKnownPlayerPosition {
    pub level: LevelId,
    pub cell: IVec2,
    pub observed_at: u64,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PersistentId(pub u64);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StableActorId(pub ActorId);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StableItemId(pub ItemId);

#[derive(Resource, Default, Debug, Clone)]
pub struct StableEntityIndex {
    actors: HashMap<ActorId, Entity>,
    items: HashMap<ItemId, Entity>,
}

impl StableEntityIndex {
    pub fn clear(&mut self) {
        self.actors.clear();
        self.items.clear();
    }

    pub fn insert_actor(&mut self, id: ActorId, entity: Entity) {
        let previous = self.actors.insert(id, entity);
        assert!(previous.is_none(), "duplicate stable actor id {}", id.raw());
    }

    pub fn insert_item(&mut self, id: ItemId, entity: Entity) {
        let previous = self.items.insert(id, entity);
        assert!(previous.is_none(), "duplicate stable item id {}", id.raw());
    }

    pub fn actor(&self, id: ActorId) -> Option<Entity> {
        self.actors.get(&id).copied()
    }

    pub fn item(&self, id: ItemId) -> Option<Entity> {
        self.items.get(&id).copied()
    }
}

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersistentIdAllocator {
    next_id: u64,
}

impl Default for PersistentIdAllocator {
    fn default() -> Self {
        Self { next_id: 1 }
    }
}

impl PersistentIdAllocator {
    pub fn allocate(&mut self) -> Result<PersistentId, PersistentIdAllocationError> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(PersistentIdAllocationError::Exhausted)?;
        Ok(PersistentId(id))
    }

    pub fn next_available(&self) -> u64 {
        self.next_id
    }

    pub fn set_next_available(&mut self, next_id: u64) {
        self.next_id = next_id.max(1);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistentIdAllocationError {
    Exhausted,
}

#[derive(Component, Debug, Clone, Default)]
pub enum AiGoal {
    #[default]
    Idle,
    Wander,
    Investigate(GridPosition),
    Chase(ActorId),
    Flee(ActorId),
}
