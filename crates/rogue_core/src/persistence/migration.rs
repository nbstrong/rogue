use crate::persistence::snapshot::GameSnapshot;

pub const CURRENT_SAVE_VERSION: u32 = 1;

pub fn migrate_snapshot(snapshot: GameSnapshot) -> Result<GameSnapshot, String> {
    match snapshot.version {
        CURRENT_SAVE_VERSION => Ok(snapshot),
        version if version > CURRENT_SAVE_VERSION => Err(format!(
            "snapshot version {} is newer than supported version {}",
            version, CURRENT_SAVE_VERSION
        )),
        0 => Err("snapshot version 0 is unsupported".to_string()),
        version => Err(format!("unsupported snapshot version {}", version)),
    }
}
