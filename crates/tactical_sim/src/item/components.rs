use bevy_ecs::prelude::*;

use crate::actor::components::{ActorId, ItemId};

#[derive(Component)]
pub struct Item;

#[derive(Component, Debug, Clone)]
pub struct Inventory {
    pub capacity: usize,
    pub items: Vec<ItemId>,
}

impl Inventory {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            items: Vec::new(),
        }
    }

    pub fn is_full(&self) -> bool {
        self.items.len() >= self.capacity
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CarriedBy(pub ActorId);
