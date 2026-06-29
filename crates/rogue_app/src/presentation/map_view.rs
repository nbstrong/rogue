use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy::text::{Justify, TextColor, TextFont, TextLayout};
use bevy_math::{IVec2, Vec3};
use rogue_core::world::map::LevelId;
use rogue_core::world::map::LevelMap;
use rogue_core::world::tile::TileKind;

use crate::game::{MapTileView, MapViews, SessionEntity, TILE_SIZE};

fn tile_glyph(tile: &TileKind) -> &'static str {
    match tile {
        TileKind::Floor => ".",
        TileKind::Wall => "#",
        TileKind::ClosedDoor => "+",
        TileKind::OpenDoor => "/",
        TileKind::StairsUp => "<",
        TileKind::StairsDown => ">",
    }
}

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
    let text_font = TextFont::from_font_size(TILE_SIZE);

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
            let glyph = tile_glyph(&tile.kind);
            let visibility = if tile.visible || tile.explored {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };

            if let Some(entity) = views.tiles.get(&key).copied() {
                commands.entity(entity).insert((
                    Text2d::new(glyph),
                    text_font.clone(),
                    TextColor(color),
                    Anchor::CENTER,
                    TextLayout::justify(Justify::Center),
                    Transform::from_translation(position),
                    visibility,
                ));
            } else {
                let entity = commands
                    .spawn((
                        Text2d::new(glyph),
                        text_font.clone(),
                        TextColor(color),
                        Anchor::CENTER,
                        TextLayout::justify(Justify::Center),
                        Transform::from_translation(position),
                        visibility,
                        MapTileView,
                        SessionEntity,
                    ))
                    .id();
                views.tiles.insert(key, entity);
            }
        }
    }
}
