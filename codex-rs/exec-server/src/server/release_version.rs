//! Advertise the supported upstream executor contract independently of the fork release.

use codex_build_info::CODEX_BACKEND_COMPAT_VERSION;

use crate::protocol::EnvironmentInfo;

pub(super) fn local_environment_info() -> EnvironmentInfo {
    EnvironmentInfo {
        executor_version: CODEX_BACKEND_COMPAT_VERSION.to_string(),
        ..EnvironmentInfo::local()
    }
}
