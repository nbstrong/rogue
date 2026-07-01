pub const CURRENT_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SchemaVersion(pub u32);

pub fn validate_supported_version(version: u32) -> Result<(), String> {
    if version == 0 {
        return Err("snapshot version 0 is invalid".to_string());
    }
    if version > CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "snapshot version {} is newer than supported version {}",
            version, CURRENT_SCHEMA_VERSION
        ));
    }
    Ok(())
}
