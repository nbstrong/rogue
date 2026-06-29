use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::*;
use bevy_math::IVec2;

use crate::actor::components::{BlocksMovement, BlocksSight};
use crate::world::map::{LevelId, LevelMap};
use crate::world::map::GridPosition;

#[derive(Resource, Default, Debug, Clone)]
pub struct SpatialIndex {
    pub occupants: HashMap<(LevelId, IVec2), Vec<Entity>>,
    pub movement_blockers: HashSet<(LevelId, IVec2)>,
    pub sight_blockers: HashSet<(LevelId, IVec2)>,
}

impl SpatialIndex {
    pub fn rebuild(&mut self, entities: &Query<'_, '_, (Entity, &GridPosition, Option<&BlocksMovement>, Option<&BlocksSight>)>) {
        self.occupants.clear();
        self.movement_blockers.clear();
        self.sight_blockers.clear();

        for (entity, position, blocks_movement, blocks_sight) in entities.iter() {
            let key = (position.level, position.cell);
            self.occupants.entry(key).or_default().push(entity);
            if blocks_movement.is_some() {
                self.movement_blockers.insert(key);
            }
            if blocks_sight.is_some() {
                self.sight_blockers.insert(key);
            }
        }
    }

    pub fn occupants_at(&self, level: LevelId, cell: IVec2) -> impl Iterator<Item = Entity> + '_ {
        self.occupants
            .get(&(level, cell))
            .into_iter()
            .flat_map(|entities| entities.iter().copied())
    }
}

pub fn update_spatial_index(
    mut index: ResMut<'_, SpatialIndex>,
    entities: Query<'_, '_, (Entity, &GridPosition, Option<&BlocksMovement>, Option<&BlocksSight>)>,
    _map: Option<Res<'_, LevelMap>>,
) {
    index.rebuild(&entities);
}
