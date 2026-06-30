use serde::{Deserialize, Serialize};

use crate::persistence::snapshot::{
    ActionSnapshot, EffectSnapshot, EntitySnapshot, GameSnapshot, LevelSnapshot,
    PersistentIdAllocatorSnapshot, RandomSnapshot, ScheduledActorSnapshot,
    SimulationStatusSnapshot,
};

pub const CURRENT_SAVE_VERSION: u32 = 2;

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
#[serde(untagged)]
pub enum SnapshotFile {
    Current(GameSnapshot),
    V1(LegacyGameSnapshotV1),
}

pub fn migrate_snapshot(snapshot: SnapshotFile) -> Result<GameSnapshot, String> {
    match snapshot {
        SnapshotFile::Current(snapshot) => match snapshot.version {
            CURRENT_SAVE_VERSION => Ok(snapshot),
            version if version > CURRENT_SAVE_VERSION => Err(format!(
                "snapshot version {} is newer than supported version {}",
                version, CURRENT_SAVE_VERSION
            )),
            version => Err(format!("unsupported snapshot version {}", version)),
        },
        SnapshotFile::V1(snapshot) => migrate_v1_snapshot(snapshot),
    }
}

fn migrate_v1_snapshot(snapshot: LegacyGameSnapshotV1) -> Result<GameSnapshot, String> {
    match snapshot.version {
        1 => {
            let next_sequence = snapshot
                .timeline
                .iter()
                .map(|entry| entry.sequence)
                .max()
                .map(|sequence| sequence.saturating_add(1))
                .unwrap_or(0);

            Ok(GameSnapshot {
                version: CURRENT_SAVE_VERSION,
                root_seed: snapshot.root_seed,
                current_level: snapshot.current_level,
                current_tick: snapshot.current_tick,
                next_sequence,
                current_actor: snapshot.current_actor,
                simulation_status: snapshot.simulation_status,
                persistent_ids: snapshot.persistent_ids,
                levels: snapshot.levels,
                entities: snapshot.entities,
                timeline: snapshot.timeline,
                pending_actions: snapshot.pending_actions,
                pending_effects: snapshot.pending_effects,
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
