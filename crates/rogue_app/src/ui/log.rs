use bevy::prelude::*;
use bevy::ui::{Node, PositionType, Val};

use crate::game::{CombatLog, LogText};

pub fn flush_combat_log(
    mut commands: Commands<'_, '_>,
    mut log: ResMut<'_, CombatLog>,
    query: Query<'_, '_, Entity, With<LogText>>,
) {
    let text = log
        .lines
        .iter()
        .rev()
        .take(6)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");

    if let Some(entity) = query.iter().next() {
        commands.entity(entity).insert(Text::new(text));
    } else {
        commands.spawn((
            Text::new(text),
            TextFont::from_font_size(16.0),
            TextColor(Color::srgb(0.85, 0.85, 0.85)),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(16.0),
                bottom: Val::Px(12.0),
                ..default()
            },
            LogText,
        ));
    }

    if log.lines.len() > 24 {
        while log.lines.len() > 24 {
            let _ = log.lines.pop_front();
        }
    }
}
