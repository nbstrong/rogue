use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::*;
use bevy_math::IVec2;

use crate::actor::components::{BlocksMovement, BlocksSight, StableActorId, StableItemId};
use crate::world::map::GridPosition;
use crate::world::map::{LevelId, LevelMap};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SpatialOccupantOrder {
    Actor(u64),
    Item(u64),
    Entity(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SpatialOccupant {
    order: SpatialOccupantOrder,
    entity: Entity,
}

#[derive(Resource, Default, Debug, Clone)]
pub struct SpatialIndex {
    occupants: HashMap<(LevelId, IVec2), Vec<SpatialOccupant>>,
    pub movement_blockers: HashSet<(LevelId, IVec2)>,
    pub sight_blockers: HashSet<(LevelId, IVec2)>,
}

impl SpatialIndex {
    fn occupant_order(
        entity: Entity,
        stable_actor: Option<&StableActorId>,
        stable_item: Option<&StableItemId>,
    ) -> SpatialOccupantOrder {
        if let Some(stable_actor) = stable_actor {
            SpatialOccupantOrder::Actor(stable_actor.0.raw())
        } else if let Some(stable_item) = stable_item {
            SpatialOccupantOrder::Item(stable_item.0.raw())
        } else {
            SpatialOccupantOrder::Entity(entity.to_bits())
        }
    }

    pub fn insert_occupant(
        &mut self,
        entity: Entity,
        position: GridPosition,
        stable_actor: Option<&StableActorId>,
        stable_item: Option<&StableItemId>,
        blocks_movement: bool,
        blocks_sight: bool,
    ) {
        let key = (position.level, position.cell);
        let occupant = SpatialOccupant {
            order: Self::occupant_order(entity, stable_actor, stable_item),
            entity,
        };
        let occupants = self.occupants.entry(key).or_default();
        occupants.push(occupant);
        occupants.sort_by_key(|entry| entry.order);
        if blocks_movement {
            self.movement_blockers.insert(key);
        }
        if blocks_sight {
            self.sight_blockers.insert(key);
        }
    }

    pub fn rebuild(
        &mut self,
        entities: &Query<
            '_,
            '_,
            (
                Entity,
                &GridPosition,
                Option<&BlocksMovement>,
                Option<&BlocksSight>,
                Option<&StableActorId>,
                Option<&StableItemId>,
            ),
        >,
    ) {
        self.occupants.clear();
        self.movement_blockers.clear();
        self.sight_blockers.clear();

        for (entity, position, blocks_movement, blocks_sight, stable_actor, stable_item) in
            entities.iter()
        {
            self.insert_occupant(
                entity,
                *position,
                stable_actor,
                stable_item,
                blocks_movement.is_some(),
                blocks_sight.is_some(),
            );
        }
    }

    pub fn occupants_at(&self, level: LevelId, cell: IVec2) -> impl Iterator<Item = Entity> + '_ {
        self.occupants
            .get(&(level, cell))
            .into_iter()
            .flat_map(|entities| entities.iter().map(|entry| entry.entity))
    }
}

pub fn update_spatial_index(
    mut index: ResMut<'_, SpatialIndex>,
    entities: Query<
        '_,
        '_,
        (
            Entity,
            &GridPosition,
            Option<&BlocksMovement>,
            Option<&BlocksSight>,
            Option<&StableActorId>,
            Option<&StableItemId>,
        ),
    >,
    _map: Option<Res<'_, LevelMap>>,
) {
    index.rebuild(&entities);
}
