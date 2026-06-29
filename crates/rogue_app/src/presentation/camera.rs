use bevy::prelude::*;
use rogue_core::actor::components::Player;
use rogue_core::world::map::{GridPosition, LevelMap};

use crate::game::TILE_SIZE;

pub fn update_camera(
    mut camera: Query<'_, '_, &mut Transform, (With<Camera2d>, Without<Player>)>,
    player: Query<'_, '_, &GridPosition, With<Player>>,
    map: Res<'_, LevelMap>,
) {
    let Some(player_position) = player.iter().next() else {
        return;
    };

    let Some(mut transform) = camera.iter_mut().next() else {
        return;
    };

    let half_width = map.width as f32 * TILE_SIZE / 2.0;
    let half_height = map.height as f32 * TILE_SIZE / 2.0;

    transform.translation = Vec3::new(
        player_position.cell.x as f32 * TILE_SIZE - half_width + TILE_SIZE / 2.0,
        player_position.cell.y as f32 * TILE_SIZE - half_height + TILE_SIZE / 2.0,
        999.0,
    );
}
