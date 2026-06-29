use std::collections::HashMap;

use bevy_ecs::prelude::*;

use crate::content::definitions::{ActorDefinition, ItemDefinition};

#[derive(Resource, Default, Debug, Clone)]
pub struct ContentRegistry {
    pub actors: HashMap<String, ActorDefinition>,
    pub items: HashMap<String, ItemDefinition>,
}

impl ContentRegistry {
    pub fn insert_actor(&mut self, definition: ActorDefinition) -> Result<(), String> {
        if self.actors.contains_key(&definition.id) {
            return Err(format!("duplicate actor id: {}", definition.id));
        }
        self.actors.insert(definition.id.clone(), definition);
        Ok(())
    }

    pub fn insert_item(&mut self, definition: ItemDefinition) -> Result<(), String> {
        if self.items.contains_key(&definition.id) {
            return Err(format!("duplicate item id: {}", definition.id));
        }
        self.items.insert(definition.id.clone(), definition);
        Ok(())
    }
}

