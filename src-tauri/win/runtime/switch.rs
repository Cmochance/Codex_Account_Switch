use std::path::Path;

use crate::errors::AppResult;
use crate::models::SwitchResponse;
use crate::{platform, shared::switch_core};

// The Windows-only pre-switch quota steps that used to live here moved
// into shared switch_core (live-quota fold + root metadata refresh); the
// target-prime step was removed outright — activation-scoped session
// filtering closes the cross-account display bleed it guarded against,
// and priming the stored timestamp to "now" only suppressed the
// immediate post-switch API refresh behind the 5-min staleness gate.

pub fn switch_profile_with_home(
    profile_name: &str,
    codex_home: Option<&Path>,
) -> AppResult<SwitchResponse> {
    switch_core::switch_profile_with_home(platform::current_hooks(), profile_name, codex_home)
}

pub fn switch_profile(profile_name: &str) -> AppResult<SwitchResponse> {
    switch_profile_with_home(profile_name, None)
}
