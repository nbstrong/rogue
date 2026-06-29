use bevy::prelude::*;
use rogue_core::actor::components::Player;
use rogue_core::world::map::GridPosition;

use crate::game::TILE_SIZE;

pub fn update_camera(
    mut camera: Query<'_, '_, &mut Transform, (With<Camera2d>, Without<Player>)>,
    player: Query<'_, '_, &GridPosition, With<Player>>,
) {
    let Some(player_position) = player.iter().next() else {
        return;
    };

    let Some(mut transform) = camera.iter_mut().next() else {
        return;
    };

    transform.translation.x = player_position.cell.x as f32 * TILE_SIZE;
    transform.translation.y = player_position.cell.y as f32 * TILE_SIZE;
    transform.translation.z = 999.0;
}
