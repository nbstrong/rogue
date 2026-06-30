use bevy_ecs::prelude::*;
use bevy_math::IVec2;

use crate::actor::components::*;
use crate::content::definitions::ActorDefinition;
use crate::item::components::Inventory;
use crate::world::map::{GridPosition, LevelId};

pub fn spawn_player(
    commands: &mut Commands<'_, '_>,
    allocator: &mut PersistentIdAllocator,
    level: LevelId,
    cell: IVec2,
) -> Entity {
    let persistent_id = allocator.allocate();
    commands
        .spawn((
            Actor,
            Player,
            BlocksMovement,
            BlocksSight,
            Health {
                current: 10,
                maximum: 10,
            },
            ActiveStatuses::default(),
            CombatStats {
                power: 3,
                defense: 1,
            },
            Vision { range: 8 },
            ActionSpeed {
                ticks_per_action: 100,
            },
            Inventory::new(8),
            PrototypeId("player".to_string()),
            GridPosition { level, cell },
            persistent_id,
        ))
        .id()
}

pub fn spawn_monster(
    commands: &mut Commands<'_, '_>,
    allocator: &mut PersistentIdAllocator,
    definition: &ActorDefinition,
    level: LevelId,
    cell: IVec2,
) -> Entity {
    let persistent_id = allocator.allocate();
    commands
        .spawn((
            Actor,
            Monster,
            BlocksMovement,
            BlocksSight,
            HostileToPlayer,
            Health {
                current: definition.maximum_health,
                maximum: definition.maximum_health,
            },
            ActiveStatuses::default(),
            CombatStats {
                power: definition.power,
                defense: definition.defense,
            },
            Vision {
                range: definition.vision_range,
            },
            ActionSpeed {
                ticks_per_action: definition.action_speed,
            },
            PrototypeId(definition.id.clone()),
            GridPosition { level, cell },
            persistent_id,
        ))
        .id()
}

pub fn spawn_vertical_slice(commands: &mut Commands<'_, '_>) -> (Entity, Entity) {
    let mut allocator = PersistentIdAllocator::default();
    let level = LevelId(0);
    let player = spawn_player(commands, &mut allocator, level, IVec2::new(2, 2));
    let ogre = ActorDefinition {
        id: "ogre".to_string(),
        name: "Ogre".to_string(),
        glyph: 'O',
        maximum_health: 6,
        power: 2,
        defense: 0,
        vision_range: 8,
        action_speed: 120,
    };
    let monster = spawn_monster(commands, &mut allocator, &ogre, level, IVec2::new(5, 2));
    (player, monster)
}
