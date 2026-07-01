use bevy::prelude::*;
use tactical_sim::simulation::SimulationStatus;

use crate::game::{CombatLog, LogText};

pub fn flush_combat_log(
    mut query: Query<'_, '_, &mut Text, With<LogText>>,
    mut log: ResMut<'_, CombatLog>,
    status: Res<'_, SimulationStatus>,
) {
    let mut lines: Vec<_> = log.lines.iter().cloned().collect();
    if *status == SimulationStatus::Terminal {
        lines.push("Game over. Press R to restart.".to_string());
    }

    if let Some(mut text) = query.iter_mut().next() {
        *text = Text::new(lines.join("\n"));
    } else {
        return;
    }

    if *status != SimulationStatus::Terminal && log.lines.len() > 128 {
        while log.lines.len() > 128 {
            let _ = log.lines.pop_front();
        }
    }
}
