use std::fs;
use std::path::{Path, PathBuf};

use crate::game::{ActorViews, CombatLog, HealthSnapshot, MapViews};
use bevy::prelude::*;
use rogue_core::persistence::migration::migrate_snapshot;
use rogue_core::persistence::snapshot::{
    GameSnapshot, restore_world, snapshot_from_text, snapshot_to_text, snapshot_world,
};

#[derive(Resource, Debug, Clone)]
pub struct SaveFileLocation {
    pub path: PathBuf,
}

impl Default for SaveFileLocation {
    fn default() -> Self {
        Self {
            path: PathBuf::from("save/latest.ron"),
        }
    }
}

pub fn bootstrap_save_system(world: &mut World) {
    if !world.contains_resource::<SaveFileLocation>() {
        world.insert_resource(SaveFileLocation::default());
    }
}

pub fn save_world_to_path(world: &World, path: impl AsRef<Path>) -> Result<(), String> {
    let snapshot = snapshot_world(world)?;
    let text = snapshot_to_text(&snapshot)?;
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let mut temp_path = path.to_path_buf();
    temp_path.set_extension("ron.tmp");

    fs::write(&temp_path, text).map_err(|err| err.to_string())?;
    fs::rename(&temp_path, path).map_err(|err| err.to_string())?;
    Ok(())
}

pub fn load_world_from_path(world: &mut World, path: impl AsRef<Path>) -> Result<(), String> {
    let text = fs::read_to_string(path.as_ref()).map_err(|err| err.to_string())?;
    let snapshot: GameSnapshot = snapshot_from_text(&text)?;
    let snapshot = migrate_snapshot(snapshot)?;
    restore_world(world, &snapshot)?;
    if let Some(mut views) = world.get_resource_mut::<MapViews>() {
        views.tiles.clear();
    }
    if let Some(mut views) = world.get_resource_mut::<ActorViews>() {
        views.views.clear();
    }
    if let Some(mut log) = world.get_resource_mut::<CombatLog>() {
        log.lines.clear();
    }
    if let Some(mut health) = world.get_resource_mut::<HealthSnapshot>() {
        health.values.clear();
    }
    Ok(())
}

pub fn save_game(world: &World) -> Result<(), String> {
    let path = world
        .get_resource::<SaveFileLocation>()
        .map(|location| location.path.clone())
        .unwrap_or_else(|| SaveFileLocation::default().path);
    save_world_to_path(world, path)
}

pub fn load_game(world: &mut World) -> Result<(), String> {
    let path = world
        .get_resource::<SaveFileLocation>()
        .map(|location| location.path.clone())
        .unwrap_or_else(|| SaveFileLocation::default().path);
    load_world_from_path(world, path)
}
