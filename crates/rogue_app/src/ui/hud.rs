use bevy::prelude::*;
use bevy::ui::{Node, PositionType, Val};
use rogue_core::actor::components::{Health, Player};
use rogue_core::simulation::SimulationStatus;
use rogue_core::world::map::GridPosition;

use crate::game::{CombatLog, HudText};

pub fn update_hud(
    mut commands: Commands<'_, '_>,
    query: Query<'_, '_, (Entity, &Text), With<HudText>>,
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
        message.push_str("\nGame over. Press R to restart.");
    } else {
        message.push_str("\nMove with HJKL/YUBN or Space to wait.");
    }

    if let Some((entity, _)) = query.iter().next() {
        commands.entity(entity).insert(Text::new(message));
    } else {
        commands.spawn((
            Text::new(message),
            TextFont::from_font_size(18.0),
            TextColor(Color::WHITE),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(16.0),
                top: Val::Px(12.0),
                ..default()
            },
            HudText,
        ));
    }

    if log.lines.is_empty() {
        log.lines
            .push_back("A new delver enters the room.".to_string());
    }
}
