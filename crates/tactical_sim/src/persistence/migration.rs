use serde::{Deserialize, Serialize};

use crate::persistence::rng::RandomSnapshot;
use crate::persistence::snapshot::{
    ActionSnapshot, EffectSnapshot, EntitySnapshot, GameSnapshot, LevelSnapshot,
    PersistentIdAllocatorSnapshot, ScheduledActorSnapshot, SimulationStatusSnapshot,
};
use crate::simulation::SimulationDriverState;
use sim_core::persistence::version::CURRENT_SCHEMA_VERSION;

pub const CURRENT_SAVE_VERSION: u32 = CURRENT_SCHEMA_VERSION;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LegacyGameSnapshotV1 {
    pub version: u32,
    pub root_seed: u64,
    pub current_level: u32,
    pub current_tick: u64,
    pub current_actor: Option<u64>,
    pub simulation_status: SimulationStatusSnapshot,
    pub persistent_ids: PersistentIdAllocatorSnapshot,
    pub levels: Vec<LevelSnapshot>,
    pub entities: Vec<EntitySnapshot>,
    pub timeline: Vec<ScheduledActorSnapshot>,
    pub pending_actions: Vec<ActionSnapshot>,
    pub pending_effects: Vec<EffectSnapshot>,
    pub rng: RandomSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LegacyGameSnapshotV2 {
    pub version: u32,
    pub root_seed: u64,
    pub current_level: u32,
    pub current_tick: u64,
    pub next_sequence: u64,
    pub current_actor: Option<u64>,
    pub simulation_status: SimulationStatusSnapshot,
    pub persistent_ids: PersistentIdAllocatorSnapshot,
    pub levels: Vec<LevelSnapshot>,
    pub entities: Vec<EntitySnapshot>,
    pub timeline: Vec<ScheduledActorSnapshot>,
    pub pending_actions: Vec<ActionSnapshot>,
    pub pending_effects: Vec<EffectSnapshot>,
    pub rng: RandomSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum SnapshotFile {
    Current(GameSnapshot),
    V2(LegacyGameSnapshotV2),
    V1(LegacyGameSnapshotV1),
}

pub fn migrate_snapshot(snapshot: SnapshotFile) -> Result<GameSnapshot, String> {
    match snapshot {
        SnapshotFile::Current(snapshot) => match snapshot.version {
            CURRENT_SAVE_VERSION => Ok(snapshot),
            2 => Ok(GameSnapshot {
                version: CURRENT_SAVE_VERSION,
                ..snapshot
            }),
            version if version > CURRENT_SAVE_VERSION => Err(format!(
                "snapshot version {} is newer than supported version {}",
                version, CURRENT_SAVE_VERSION
            )),
            version => Err(format!("unsupported snapshot version {}", version)),
        },
        SnapshotFile::V2(snapshot) => migrate_v2_snapshot(snapshot),
        SnapshotFile::V1(snapshot) => migrate_v1_snapshot(snapshot),
    }
}

fn migrate_v2_snapshot(snapshot: LegacyGameSnapshotV2) -> Result<GameSnapshot, String> {
    match snapshot.version {
        2 => {
            let current_tick = snapshot.current_tick;
            let simulation_status = snapshot.simulation_status;
            let timeline = snapshot.timeline;
            let simulation_driver =
                snapshot_driver_from_legacy(current_tick, simulation_status, &timeline)?;
            let entities = normalize_legacy_entities(snapshot.entities);
            Ok(GameSnapshot {
                version: CURRENT_SAVE_VERSION,
                root_seed: snapshot.root_seed,
                current_level: snapshot.current_level,
                current_tick,
                next_sequence: snapshot.next_sequence,
                current_actor: snapshot.current_actor,
                simulation_status,
                persistent_ids: snapshot.persistent_ids,
                levels: snapshot.levels,
                entities,
                timeline,
                pending_actions: snapshot.pending_actions,
                pending_effects: snapshot.pending_effects,
                simulation_driver,
                rng: snapshot.rng,
            })
        }
        version if version > CURRENT_SAVE_VERSION => Err(format!(
            "snapshot version {} is newer than supported version {}",
            version, CURRENT_SAVE_VERSION
        )),
        version => Err(format!("unsupported legacy snapshot version {}", version)),
    }
}

fn migrate_v1_snapshot(snapshot: LegacyGameSnapshotV1) -> Result<GameSnapshot, String> {
    match snapshot.version {
        1 => {
            let current_tick = snapshot.current_tick;
            let simulation_status = snapshot.simulation_status;
            let timeline = snapshot.timeline;
            let next_sequence = timeline
                .iter()
                .map(|entry| entry.sequence)
                .max()
                .map(|sequence| sequence.saturating_add(1))
                .unwrap_or(0);
            let simulation_driver =
                snapshot_driver_from_legacy(current_tick, simulation_status, &timeline)?;
            let entities = normalize_legacy_entities(snapshot.entities);

            Ok(GameSnapshot {
                version: CURRENT_SAVE_VERSION,
                root_seed: snapshot.root_seed,
                current_level: snapshot.current_level,
                current_tick,
                next_sequence,
                current_actor: snapshot.current_actor,
                simulation_status,
                persistent_ids: snapshot.persistent_ids,
                levels: snapshot.levels,
                entities,
                timeline,
                pending_actions: snapshot.pending_actions,
                pending_effects: snapshot.pending_effects,
                simulation_driver,
                rng: snapshot.rng,
            })
        }
        version if version > CURRENT_SAVE_VERSION => Err(format!(
            "snapshot version {} is newer than supported version {}",
            version, CURRENT_SAVE_VERSION
        )),
        version => Err(format!("unsupported legacy snapshot version {}", version)),
    }
}

fn snapshot_driver_from_legacy(
    _current_tick: u64,
    _simulation_status: SimulationStatusSnapshot,
    _timeline: &[crate::persistence::snapshot::ScheduledActorSnapshot],
) -> Result<SimulationDriverState, String> {
    let mut simulation_driver = SimulationDriverState::default();
    simulation_driver.driver.progress = Default::default();
    simulation_driver.driver.backlog.clear();
    Ok(simulation_driver)
}

fn normalize_legacy_entities(mut entities: Vec<EntitySnapshot>) -> Vec<EntitySnapshot> {
    for entity in &mut entities {
        entity.prototype = match entity.prototype.as_str() {
            "player" => "controlled_actor".to_string(),
            "ogre" => "hostile_actor".to_string(),
            other => other.to_string(),
        };
    }
    entities
}
