use std::path::Path;

use crate::errors::AppResult;
use crate::models::SwitchResponse;
use crate::{platform, shared::switch_core};

pub fn switch_profile_with_home(
    profile_name: &str,
    codex_home: Option<&Path>,
) -> AppResult<SwitchResponse> {
    switch_core::switch_profile_with_home(platform::current_hooks(), profile_name, codex_home)
}

#[allow(dead_code)]
pub fn switch_profile(profile_name: &str) -> AppResult<SwitchResponse> {
    switch_profile_with_home(profile_name, None)
}
