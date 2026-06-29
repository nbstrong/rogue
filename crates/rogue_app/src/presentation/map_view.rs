use bevy::prelude::*;
use bevy::sprite::Sprite;
use bevy_math::{IVec2, Vec3};
use rogue_core::world::map::LevelId;
use rogue_core::world::map::LevelMap;
use rogue_core::world::tile::TileKind;

use crate::game::{MapTileView, MapViews, SessionEntity, TILE_SIZE};

fn tile_color(tile: &TileKind, visible: bool, explored: bool) -> Color {
    let base = match tile {
        TileKind::Floor => Color::srgb(0.18, 0.18, 0.20),
        TileKind::Wall => Color::srgb(0.38, 0.24, 0.14),
        TileKind::ClosedDoor => Color::srgb(0.54, 0.40, 0.18),
        TileKind::OpenDoor => Color::srgb(0.30, 0.22, 0.10),
        TileKind::StairsUp => Color::srgb(0.38, 0.48, 0.65),
        TileKind::StairsDown => Color::srgb(0.20, 0.36, 0.24),
    };

    if visible {
        base
    } else if explored {
        base.with_alpha(0.45)
    } else {
        Color::srgb(0.02, 0.02, 0.03).with_alpha(0.0)
    }
}

pub fn synchronize_map_view(
    mut commands: Commands<'_, '_>,
    mut views: ResMut<'_, MapViews>,
    map: Res<'_, LevelMap>,
) {
    let half_w = map.width as f32 * TILE_SIZE / 2.0;
    let half_h = map.height as f32 * TILE_SIZE / 2.0;

    for y in 0..map.height as i32 {
        for x in 0..map.width as i32 {
            let cell = IVec2::new(x, y);
            let tile = map.tile(cell).expect("map tile");
            let key = (LevelId(0), cell);
            let position = Vec3::new(
                x as f32 * TILE_SIZE - half_w + TILE_SIZE / 2.0,
                y as f32 * TILE_SIZE - half_h + TILE_SIZE / 2.0,
                0.0,
            );
            let color = tile_color(&tile.kind, tile.visible, tile.explored);

            if let Some(entity) = views.tiles.get(&key).copied() {
                commands.entity(entity).insert((
                    Transform::from_translation(position),
                    Sprite::from_color(color, Vec2::splat(TILE_SIZE - 1.0)),
                    Visibility::Visible,
                ));
            } else {
                let entity = commands
                    .spawn((
                        Sprite::from_color(color, Vec2::splat(TILE_SIZE - 1.0)),
                        Transform::from_translation(position),
                        MapTileView,
                        SessionEntity,
                    ))
                    .id();
                views.tiles.insert(key, entity);
            }
        }
    }
}
