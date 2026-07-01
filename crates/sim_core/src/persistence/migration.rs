use super::version::validate_supported_version;

pub fn validate_current_version(version: u32) -> Result<(), String> {
    validate_supported_version(version)
}
