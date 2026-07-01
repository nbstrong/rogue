use std::collections::HashSet;

use bevy_ecs::prelude::*;
use bevy_math::IVec2;
use tactical_sim::action::queue::ActionQueue;
use tactical_sim::action::resolver::{ActionDecision, ActionOutcomeLog};
use tactical_sim::actor::components::{
    ActionSpeed, ActiveStatuses, Actor, BlocksMovement, BlocksSight, CombatStats, Health,
    HostileToPlayer, Monster, PersistentId, PersistentIdAllocator, Player, PrototypeId,
    StableActorId, StableEntityIndex, StableItemId, Vision,
};
use tactical_sim::content::definitions::ActorDefinition;
use tactical_sim::content::registry::ContentRegistry;
use tactical_sim::item::components::{Inventory, Item};
use tactical_sim::item::effects::EffectQueue;
use tactical_sim::persistence::rng::RandomStreams;
use tactical_sim::simulation::{SimulationDriverState, SimulationStatus};
use tactical_sim::time::clock::{CurrentActor, TurnClock};
use tactical_sim::world::fov::recalculate_fov_for_player;
use tactical_sim::world::generation::generate_one_room_with_rng;
use tactical_sim::world::map::{GridPosition, LevelId, LevelMap};
use tactical_sim::world::spatial::SpatialIndex;

#[derive(Component)]
struct SessionEntity;

pub fn setup_new_game(world: &mut World, clear_existing: bool) {
    if clear_existing {
        let mut cleanup = HashSet::new();
        for entity in world
            .query_filtered::<Entity, With<SessionEntity>>()
            .iter(world)
        {
            cleanup.insert(entity);
        }
        for entity in world
            .query_filtered::<Entity, With<PersistentId>>()
            .iter(world)
        {
            cleanup.insert(entity);
        }
        for entity in cleanup {
            let _ = world.despawn(entity);
        }
    }

    world.remove_resource::<LevelMap>();
    world.remove_resource::<SpatialIndex>();
    world.remove_resource::<ActionQueue>();
    world.remove_resource::<EffectQueue>();
    world.remove_resource::<TurnClock>();
    world.remove_resource::<SimulationStatus>();
    world.remove_resource::<ActionDecision>();
    world.remove_resource::<ActionOutcomeLog>();
    world.remove_resource::<CurrentActor>();
    world.remove_resource::<StableEntityIndex>();
    world.remove_resource::<SimulationDriverState>();
    world.insert_resource(RandomStreams::seeded(0));
    world.insert_resource(PersistentIdAllocator::default());
    world.insert_resource(StableEntityIndex::default());
    world.insert_resource(SimulationDriverState::default());

    let player_def = world
        .resource::<ContentRegistry>()
        .actors
        .get("player")
        .cloned()
        .unwrap_or_else(|| panic!("missing player definition"));
    let monster_def = world
        .resource::<ContentRegistry>()
        .actors
        .get("ogre")
        .cloned()
        .unwrap_or_else(|| panic!("missing ogre definition"));

    let level = LevelId(0);
    let mut map = {
        let mut rng = world.resource_mut::<RandomStreams>();
        generate_one_room_with_rng(21, 15, Some(&mut *rng))
    };
    let player_cell = IVec2::new(3, 7);
    let monster_cell = IVec2::new(8, 7);

    let player = spawn_actor(world, &player_def, level, player_cell, true, false);
    let monster = spawn_actor(world, &monster_def, level, monster_cell, false, true);

    let loot_cell = {
        let mut rng = world.resource_mut::<RandomStreams>();
        let interior_width = map.width as usize - 2;
        let interior_height = map.height as usize - 2;
        let total_cells = interior_width * interior_height;
        let mut candidate_index = (rng.next_generation_u64() as usize) % total_cells;
        let mut cell = IVec2::new(1, 1);
        for _ in 0..total_cells {
            let x = 1 + (candidate_index % interior_width) as i32;
            let y = 1 + (candidate_index / interior_width) as i32;
            cell = IVec2::new(x, y);
            if cell != player_cell && cell != monster_cell {
                break;
            }
            candidate_index = (candidate_index + 1) % total_cells;
        }
        cell
    };
    let loot_name = {
        let mut rng = world.resource_mut::<RandomStreams>();
        if rng.next_loot_u64() & 1 == 0 {
            "healing_potion"
        } else {
            "trinket"
        }
    };
    let loot = spawn_loot_item(world, level, loot_cell, loot_name);
    let player_stable_id = world.entity(player).get::<StableActorId>().copied();
    let monster_stable_id = world.entity(monster).get::<StableActorId>().copied();
    let loot_stable_id = world.entity(loot).get::<StableItemId>().copied();

    let mut spatial = SpatialIndex::default();
    insert_occupant(
        &mut spatial,
        player_stable_id.as_ref(),
        None,
        world.entity(player).get::<PersistentId>().copied(),
        level,
        player_cell,
        player,
        true,
        true,
    );
    insert_occupant(
        &mut spatial,
        monster_stable_id.as_ref(),
        None,
        world.entity(monster).get::<PersistentId>().copied(),
        level,
        monster_cell,
        monster,
        true,
        true,
    );
    insert_occupant(
        &mut spatial,
        None,
        loot_stable_id.as_ref(),
        world.entity(loot).get::<PersistentId>().copied(),
        level,
        loot_cell,
        loot,
        false,
        false,
    );

    if let Some((_, vision)) = world
        .query_filtered::<(&GridPosition, &Vision), With<Player>>()
        .iter(world)
        .next()
    {
        recalculate_fov_for_player(
            &mut map,
            &spatial,
            GridPosition {
                level,
                cell: player_cell,
            },
            vision.range,
        );
    }

    let mut clock = TurnClock::default();
    clock.schedule_at(
        world
            .entity(player)
            .get::<StableActorId>()
            .expect("stable player id")
            .0,
        0,
    );
    clock.schedule_at(
        world
            .entity(monster)
            .get::<StableActorId>()
            .expect("stable monster id")
            .0,
        0,
    );

    world.insert_resource(map);
    world.insert_resource(spatial);
    world.insert_resource(ActionQueue::default());
    world.insert_resource(EffectQueue::default());
    world.insert_resource(clock);
    world.insert_resource(SimulationStatus::WaitingForPlayer);
    world.insert_resource(ActionDecision::default());
    world.insert_resource(ActionOutcomeLog::default());
    world.insert_resource(CurrentActor::default());
}

pub fn register_content(
    registry: &mut ContentRegistry,
    actor_definitions: impl IntoIterator<Item = ActorDefinition>,
) {
    for actor in actor_definitions {
        registry
            .insert_actor(actor)
            .unwrap_or_else(|error| panic!("{}", error));
    }
}

fn spawn_actor(
    world: &mut World,
    definition: &ActorDefinition,
    level: LevelId,
    cell: IVec2,
    is_player: bool,
    hostile: bool,
) -> Entity {
    let persistent_id = next_persistent_id(world);
    let stable_actor_id = tactical_sim::ActorId::new(persistent_id).expect("valid actor id");
    let entity_id = {
        let mut entity = world.spawn((
            Actor,
            BlocksMovement,
            BlocksSight,
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
            PersistentId(persistent_id),
            StableActorId(stable_actor_id),
            SessionEntity,
        ));

        if is_player {
            entity.insert(Player);
            entity.insert(Inventory::new(8));
        }
        if hostile {
            entity.insert((Monster, HostileToPlayer));
        }

        entity.id()
    };

    if let Some(mut index) = world.get_resource_mut::<StableEntityIndex>() {
        index.insert_actor(stable_actor_id, entity_id);
    }

    entity_id
}

fn spawn_loot_item(world: &mut World, level: LevelId, cell: IVec2, prototype: &str) -> Entity {
    let persistent_id = next_persistent_id(world);
    let stable_item_id = tactical_sim::ItemId::new(persistent_id).expect("valid item id");
    let entity = world.spawn((
        Item,
        PrototypeId(prototype.to_string()),
        GridPosition { level, cell },
        PersistentId(persistent_id),
        StableItemId(stable_item_id),
        SessionEntity,
    ));
    let entity_id = entity.id();

    if let Some(mut index) = world.get_resource_mut::<StableEntityIndex>() {
        index.insert_item(stable_item_id, entity_id);
    }

    entity_id
}

fn next_persistent_id(world: &mut World) -> u64 {
    let mut allocator = world
        .get_resource_mut::<PersistentIdAllocator>()
        .expect("persistent id allocator");
    allocator
        .allocate()
        .expect("persistent id allocator exhausted")
        .0
}

fn insert_occupant(
    spatial: &mut SpatialIndex,
    stable_actor: Option<&StableActorId>,
    stable_item: Option<&StableItemId>,
    persistent_id: Option<PersistentId>,
    level: LevelId,
    cell: IVec2,
    entity: Entity,
    blocks_movement: bool,
    blocks_sight: bool,
) {
    let position = GridPosition { level, cell };
    spatial.insert_occupant(
        entity,
        position,
        stable_actor,
        stable_item,
        persistent_id.as_ref(),
        blocks_movement,
        blocks_sight,
    );
}
