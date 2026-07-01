use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::game::{ActorViews, CombatLog, HealthSnapshot, MapViews};
use bevy::prelude::*;
use tactical_sim::persistence::migration::SnapshotFile;
use tactical_sim::persistence::migration::migrate_snapshot;
use tactical_sim::persistence::snapshot::{
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
    let parent = normalized_parent(path);
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
    fs::create_dir_all(normalized_parent(path)).map_err(|err| err.to_string())?;

    let (temp_path, mut temp_file) = create_unique_temp_file(path)?;
    let write_result = temp_file
        .write_all(text.as_bytes())
        .and_then(|_| temp_file.flush())
        .and_then(|_| temp_file.sync_all());
    drop(temp_file);
    if let Err(err) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(err.to_string());
    }

    let commit_result = commit(&temp_path, path);
    if let Err(err) = commit_result {
        let _ = fs::remove_file(&temp_path);
        return Err(err.to_string());
    }

    sync_parent_directory(&normalized_parent(path))?;
    Ok(())
}

pub fn save_world_to_path(world: &World, path: impl AsRef<Path>) -> Result<(), String> {
    save_world_to_path_impl(world, path.as_ref(), replace_file_atomic)
}

fn sync_parent_directory(parent: &Path) -> Result<(), String> {
    #[cfg(not(target_os = "windows"))]
    {
        let dir = fs::File::open(parent).map_err(|err| err.to_string())?;
        dir.sync_all().map_err(|err| err.to_string())
    }

    #[cfg(target_os = "windows")]
    {
        let _ = parent;
        Ok(())
    }
}

fn normalized_parent(path: &Path) -> PathBuf {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_file_atomic(temp: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temp, destination)
}

#[cfg(target_os = "windows")]
fn replace_file_atomic(temp: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(
            lpExistingFileName: *const u16,
            lpNewFileName: *const u16,
            dwFlags: u32,
        ) -> i32;
    }

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    let temp_wide: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let ok = unsafe {
        MoveFileExW(
            temp_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };

    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
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
    use tactical_sim::action::queue::ActionQueue;
    use tactical_sim::action::resolver::ActionDecision;
    use tactical_sim::actor::components::{
        ActionSpeed, Actor, BlocksMovement, BlocksSight, CombatStats, Health, PersistentId,
        PersistentIdAllocator, PrototypeId, StableActorId, Vision,
    };
    use tactical_sim::item::effects::EffectQueue;
    use tactical_sim::persistence::rng::RandomStreams;
    use tactical_sim::simulation::{SimulationDriverState, SimulationStatus};
    use tactical_sim::time::clock::{CurrentActor, TurnClock};
    use tactical_sim::world::generation::generate_one_room;
    use tactical_sim::world::map::{GridPosition, LevelId};
    use tactical_sim::world::spatial::SpatialIndex;

    fn build_world() -> World {
        let mut world = World::new();
        world.insert_resource(generate_one_room(5, 5));
        world.insert_resource(SpatialIndex::default());
        world.insert_resource(RandomStreams::seeded(0));
        let mut allocator = PersistentIdAllocator::default();
        allocator.set_next_available(2);
        world.insert_resource(allocator);
        world.insert_resource(ActionQueue::default());
        world.insert_resource(EffectQueue::default());
        world.insert_resource(ActionDecision::default());
        world.insert_resource(CurrentActor::default());
        world.insert_resource(TurnClock::default());
        world.insert_resource(SimulationDriverState::default());
        world.insert_resource(SimulationStatus::WaitingForPlayer);
        let player = world
            .spawn((
                Actor,
                tactical_sim::actor::components::ControlledActor,
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
                StableActorId(tactical_sim::ActorId::new(1).expect("valid actor id")),
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

        let mut commit_called = false;
        let result = save_world_to_path_impl(&world, &path, |_temp, _destination| {
            commit_called = true;
            Err(std::io::Error::other("commit failure"))
        });
        assert!(result.is_err());
        assert!(commit_called);
        assert_eq!(
            fs::read_to_string(&path).expect("save file"),
            "original save"
        );
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn bare_relative_filename_saves_and_loads() {
        let original_dir = std::env::current_dir().expect("current dir");
        let temp_dir = std::env::temp_dir().join(format!(
            "rogue-save-bare-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).expect("temp dir");
        std::env::set_current_dir(&temp_dir).expect("enter temp dir");

        let world = build_world();
        let save_name = PathBuf::from("bare-save.ron");
        save_world_to_path(&world, &save_name).expect("save bare filename");

        let mut loaded = World::new();
        loaded.insert_resource(generate_one_room(5, 5));
        loaded.insert_resource(SpatialIndex::default());
        loaded.insert_resource(RandomStreams::seeded(0));
        loaded.insert_resource(PersistentIdAllocator::default());
        loaded.insert_resource(ActionQueue::default());
        loaded.insert_resource(EffectQueue::default());
        loaded.insert_resource(ActionDecision::default());
        loaded.insert_resource(CurrentActor::default());
        loaded.insert_resource(TurnClock::default());
        loaded.insert_resource(SimulationDriverState::default());
        loaded.insert_resource(SimulationStatus::WaitingForPlayer);

        load_world_from_path(&mut loaded, &save_name).expect("load bare filename");

        std::env::set_current_dir(&original_dir).expect("restore dir");
        let _ = fs::remove_file(temp_dir.join("bare-save.ron"));
        let _ = fs::remove_dir_all(temp_dir);
    }
}
