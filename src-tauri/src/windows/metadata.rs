use std::fs;
use std::path::Path;

use crate::models::ProfileMetadata;

use super::paths::{get_profile_metadata_path, validate_profile_name};

pub fn load_profile_metadata(profile_name: &str, codex_home: Option<&Path>) -> ProfileMetadata {
    let profile_name = match validate_profile_name(profile_name) {
        Ok(value) => value,
        Err(_) => return ProfileMetadata::with_folder_name(profile_name),
    };

    let metadata_path = get_profile_metadata_path(&profile_name, codex_home);
    let default_metadata = || ProfileMetadata::with_folder_name(&profile_name);

    let raw = match fs::read_to_string(metadata_path) {
        Ok(value) => value,
        Err(_) => return default_metadata(),
    };

    let mut metadata = match serde_json::from_str::<ProfileMetadata>(&raw)
        .ok()
        .and_then(ProfileMetadata::validate)
    {
        Some(value) => value,
        None => return default_metadata(),
    };

    if metadata.folder_name.is_none() {
        metadata.folder_name = Some(profile_name);
    }

    metadata
}

pub fn save_profile_metadata(
    profile_name: &str,
    metadata: &ProfileMetadata,
    codex_home: Option<&Path>,
) -> Result<(), crate::errors::AppError> {
    let profile_name = validate_profile_name(profile_name)?;
    let metadata_path = get_profile_metadata_path(&profile_name, codex_home);
    let serialized = serde_json::to_string_pretty(metadata).map_err(|error| {
        crate::errors::AppError::new(
            "PROFILE_METADATA_INVALID",
            format!("Failed to serialize metadata: {error}"),
        )
    })?;

    fs::write(metadata_path, format!("{serialized}\n")).map_err(|error| {
        crate::errors::AppError::new(
            "PROFILE_METADATA_WRITE_FAILED",
            format!("Failed to write metadata: {error}"),
        )
    })
}
