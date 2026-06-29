use bevy_ecs::prelude::*;
use bevy_math::IVec2;

use crate::world::map::GridPosition;
use crate::world::map::LevelMap;
use crate::world::tile::TileKind;

fn line_of_sight(map: &LevelMap, from: IVec2, to: IVec2) -> bool {
    let mut x0 = from.x;
    let mut y0 = from.y;
    let x1 = to.x;
    let y1 = to.y;

    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        if x0 == x1 && y0 == y1 {
            return true;
        }

        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }

        let pos = IVec2::new(x0, y0);
        if pos != to {
            match map.tile(pos).map(|tile| tile.kind) {
                Some(TileKind::Wall) | Some(TileKind::ClosedDoor) => return false,
                Some(_) => {}
                None => return false,
            }
        }
    }
}

pub fn recalculate_fov_for_player(map: &mut LevelMap, player_position: GridPosition) {
    let origin = player_position.cell;

    for tile in &mut map.tiles {
        tile.visible = false;
    }

    let range = 8;

    for y in 0..map.height as i32 {
        for x in 0..map.width as i32 {
            let cell = IVec2::new(x, y);
            let delta = cell - origin;
            if delta.x.abs().max(delta.y.abs()) > range {
                continue;
            }
            if line_of_sight(&map, origin, cell) {
                if let Some(tile) = map.tile_mut(cell) {
                    tile.visible = true;
                    tile.explored = true;
                }
            }
        }
    }
}

pub fn recalculate_fov(
    mut map: ResMut<'_, LevelMap>,
    player: Query<'_, '_, &GridPosition, With<crate::actor::components::Player>>,
) {
    let Some(player_position) = player.iter().next().copied() else {
        return;
    };

    recalculate_fov_for_player(&mut map, player_position);
}
