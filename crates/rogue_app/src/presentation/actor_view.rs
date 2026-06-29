use bevy::prelude::*;
use bevy::sprite::Sprite;
use bevy_math::Vec3;
use rogue_core::actor::components::{Monster, Player};
use rogue_core::world::map::{GridPosition, LevelMap};

use crate::game::{ActorView, ActorViews, SessionEntity, TILE_SIZE};

fn actor_color(player: bool) -> Color {
    if player {
        Color::srgb(0.85, 0.85, 0.95)
    } else {
        Color::srgb(0.80, 0.30, 0.30)
    }
}

pub fn synchronize_actor_views(
    mut commands: Commands<'_, '_>,
    mut views: ResMut<'_, ActorViews>,
    map: Res<'_, LevelMap>,
    actors: Query<'_, '_, (Entity, &GridPosition, Option<&Player>, Option<&Monster>)>,
) {
    let half_w = map.width as f32 * TILE_SIZE / 2.0;
    let half_h = map.height as f32 * TILE_SIZE / 2.0;
    let mut seen = std::collections::HashSet::new();

    for (entity, position, player, monster) in actors.iter() {
        seen.insert(entity);
        let view_entity = views.views.entry(entity).or_insert_with(|| {
            commands
                .spawn((
                    Sprite::from_color(actor_color(player.is_some()), Vec2::splat(TILE_SIZE * 0.8)),
                    Transform::default(),
                    ActorView,
                    SessionEntity,
                ))
                .id()
        });

        let world_pos = Vec3::new(
            position.cell.x as f32 * TILE_SIZE - half_w + TILE_SIZE / 2.0,
            position.cell.y as f32 * TILE_SIZE - half_h + TILE_SIZE / 2.0,
            10.0,
        );
        let mut sprite =
            Sprite::from_color(actor_color(player.is_some()), Vec2::splat(TILE_SIZE * 0.8));
        if monster.is_some() {
            sprite.color = Color::srgb(0.80, 0.30, 0.30);
        }
        let visible = map
            .tile(position.cell)
            .map(|tile| tile.visible)
            .unwrap_or(false)
            || player.is_some();
        commands.entity(*view_entity).insert((
            Transform::from_translation(world_pos),
            sprite,
            if visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
        ));
    }

    let stale: Vec<_> = views
        .views
        .keys()
        .copied()
        .filter(|entity| !seen.contains(entity))
        .collect();
    for entity in stale {
        if let Some(view_entity) = views.views.remove(&entity) {
            commands.entity(view_entity).despawn();
        }
    }
}
