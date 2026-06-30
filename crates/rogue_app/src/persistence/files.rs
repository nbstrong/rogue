use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::game::{ActorViews, CombatLog, HealthSnapshot, MapViews};
use bevy::prelude::*;
use rogue_core::persistence::migration::SnapshotFile;
use rogue_core::persistence::migration::migrate_snapshot;
use rogue_core::persistence::snapshot::{
    restore_world, snapshot_from_text, snapshot_to_text, snapshot_world,
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

fn create_unique_temp_file(path: &Path) -> Result<(PathBuf, fs::File), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "save path does not have a parent directory".to_string())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "save path does not have a file name".to_string())?
        .to_string_lossy();
    let pid = std::process::id();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?
        .as_nanos();

    for attempt in 0..1_024u32 {
        let temp_name = format!("{file_name}.{pid}.{timestamp}.{attempt}.tmp");
        let temp_path = parent.join(temp_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.to_string()),
        }
    }

    Err("failed to create a unique temporary save file".to_string())
}

fn save_world_to_path_impl(
    world: &World,
    path: &Path,
    commit: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let snapshot = snapshot_world(world)?;
    let text = snapshot_to_text(&snapshot)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let (temp_path, mut temp_file) = create_unique_temp_file(path)?;
    temp_file
        .write_all(text.as_bytes())
        .and_then(|_| temp_file.flush())
        .and_then(|_| temp_file.sync_all())
        .map_err(|err| err.to_string())?;
    drop(temp_file);

    let commit_result = commit(&temp_path, path);
    if let Err(err) = commit_result {
        let _ = fs::remove_file(&temp_path);
        return Err(err.to_string());
    }
    Ok(())
}

pub fn save_world_to_path(world: &World, path: impl AsRef<Path>) -> Result<(), String> {
    save_world_to_path_impl(world, path.as_ref(), |temp, destination| {
        fs::rename(temp, destination)
    })
}

pub fn load_world_from_path(world: &mut World, path: impl AsRef<Path>) -> Result<(), String> {
    let text = fs::read_to_string(path.as_ref()).map_err(|err| err.to_string())?;
    let snapshot: SnapshotFile = snapshot_from_text(&text)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::prelude::World;
    use bevy_math::IVec2;
    use rogue_core::actor::components::{
        ActionSpeed, Actor, BlocksMovement, BlocksSight, CombatStats, Health, PersistentId,
        PersistentIdAllocator, Player, PrototypeId, Vision,
    };
    use rogue_core::world::generation::generate_one_room;
    use rogue_core::world::map::{GridPosition, LevelId};
    use rogue_core::world::spatial::SpatialIndex;

    fn build_world() -> World {
        let mut world = World::new();
        world.insert_resource(generate_one_room(5, 5));
        world.insert_resource(SpatialIndex::default());
        world.insert_resource(PersistentIdAllocator::default());
        let player = world
            .spawn((
                Actor,
                Player,
                BlocksMovement,
                BlocksSight,
                Health {
                    current: 10,
                    maximum: 10,
                },
                CombatStats {
                    power: 3,
                    defense: 1,
                },
                Vision { range: 8 },
                ActionSpeed {
                    ticks_per_action: 100,
                },
                PrototypeId("player".to_string()),
                GridPosition {
                    level: LevelId(0),
                    cell: IVec2::new(2, 2),
                },
                PersistentId(1),
            ))
            .id();
        let _ = player;
        world
    }

    #[test]
    fn failed_commit_preserves_the_existing_save() {
        let world = build_world();
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join(format!(
            "rogue-save-test-{}-{}.ron",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::write(&path, "original save").expect("seed file");

        let result = save_world_to_path_impl(&world, &path, |_temp, _destination| {
            Err(std::io::Error::other("commit failure"))
        });
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(&path).expect("save file"),
            "original save"
        );
        let _ = fs::remove_file(&path);
    }
}
