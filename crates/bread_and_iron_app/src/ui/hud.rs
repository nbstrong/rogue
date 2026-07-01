use bevy::prelude::*;
use tactical_sim::actor::components::{Health, Player};
use tactical_sim::simulation::SimulationStatus;
use tactical_sim::world::map::GridPosition;

use crate::game::{CombatLog, HudText};

pub fn update_hud(
    mut query: Query<'_, '_, &mut Text, With<HudText>>,
    player: Query<'_, '_, (&GridPosition, &Health), With<Player>>,
    status: Res<'_, SimulationStatus>,
    mut log: ResMut<'_, CombatLog>,
) {
    let Some((position, health)) = player.iter().next() else {
        return;
    };

    let mut message = format!(
        "HP {}/{}  Pos ({}, {})  {:?}",
        health.current, health.maximum, position.cell.x, position.cell.y, *status
    );
    if *status == SimulationStatus::GameOver {
        message.push_str("  Game over. Press R to restart.");
    }

    if let Some(mut text) = query.iter_mut().next() {
        *text = Text::new(message);
    } else {
        return;
    }

    if log.lines.is_empty() {
        log.lines
            .push_back("A new delver enters the room.".to_string());
    }
}
