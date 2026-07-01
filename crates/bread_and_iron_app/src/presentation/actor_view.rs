use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy::text::{Justify, TextColor, TextFont, TextLayout};
use bevy_math::Vec3;
use bread_and_iron::{Monster, Player};
use tactical_sim::world::map::{GridPosition, LevelMap};

use crate::game::{ActorView, ActorViews, SessionEntity, TILE_SIZE};

fn actor_color(player: bool) -> Color {
    if player {
        Color::srgb(0.85, 0.85, 0.95)
    } else {
        Color::srgb(0.80, 0.30, 0.30)
    }
}

fn actor_glyph(player: bool) -> &'static str {
    if player { "@" } else { "g" }
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
    let text_font = TextFont::from_font_size(TILE_SIZE);

    for (entity, position, player, monster) in actors.iter() {
        seen.insert(entity);
        let view_entity = views.views.entry(entity).or_insert_with(|| {
            commands
                .spawn((
                    Text2d::new(actor_glyph(player.is_some())),
                    text_font.clone(),
                    TextColor(actor_color(player.is_some())),
                    Anchor::CENTER,
                    TextLayout::justify(Justify::Center),
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
        let visible = map
            .tile(position.cell)
            .map(|tile| tile.visible)
            .unwrap_or(false)
            || player.is_some();
        commands.entity(*view_entity).insert((
            Text2d::new(actor_glyph(player.is_some())),
            text_font.clone(),
            TextColor(if monster.is_some() {
                Color::srgb(0.80, 0.30, 0.30)
            } else {
                actor_color(player.is_some())
            }),
            Anchor::CENTER,
            TextLayout::justify(Justify::Center),
            Transform::from_translation(world_pos),
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
