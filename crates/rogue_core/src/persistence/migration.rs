use crate::persistence::snapshot::GameSnapshot;

pub const CURRENT_SAVE_VERSION: u32 = 1;

pub fn migrate_snapshot(mut snapshot: GameSnapshot) -> GameSnapshot {
    if snapshot.version < CURRENT_SAVE_VERSION {
        snapshot.version = CURRENT_SAVE_VERSION;
    }
    snapshot
}
