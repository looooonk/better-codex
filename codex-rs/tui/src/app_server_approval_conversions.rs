//! Narrow conversion helpers for approval-related app-server payloads.
//!
//! The TUI mostly keeps app-server approval types intact. These helpers cover
//! the remaining cases where the UI consumes a private file-change display
//! model or needs to translate a granted permission response for outbound
//! submission.

use codex_app_server_protocol::AdditionalNetworkPermissions;
use codex_app_server_protocol::GrantedPermissionProfile;
use codex_protocol::request_permissions::RequestPermissionProfile as CoreRequestPermissionProfile;

pub(crate) fn granted_permission_profile_from_request(
    value: CoreRequestPermissionProfile,
) -> GrantedPermissionProfile {
    GrantedPermissionProfile {
        network: value.network.map(|network| AdditionalNetworkPermissions {
            enabled: network.enabled,
        }),
        file_system: value.file_system.map(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use super::granted_permission_profile_from_request;
    use codex_app_server_protocol::AdditionalFileSystemPermissions;
    use codex_app_server_protocol::AdditionalNetworkPermissions;
    use codex_app_server_protocol::FileSystemAccessMode;
    use codex_app_server_protocol::FileSystemPath;
    use codex_app_server_protocol::FileSystemSandboxEntry;
    use codex_app_server_protocol::FileSystemSpecialPath;
    use codex_app_server_protocol::GrantedPermissionProfile;
    use codex_app_server_protocol::RequestPermissionProfile;
    use codex_protocol::request_permissions::RequestPermissionProfile as CoreRequestPermissionProfile;
    use codex_utils_absolute_path::AbsolutePathBuf;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn absolute_path(path: &str) -> AbsolutePathBuf {
        AbsolutePathBuf::try_from(PathBuf::from(path)).expect("path must be absolute")
    }

    #[test]
    fn converts_request_permissions_into_granted_permissions() {
        let request = RequestPermissionProfile {
            network: Some(AdditionalNetworkPermissions {
                enabled: Some(true),
            }),
            file_system: Some(AdditionalFileSystemPermissions {
                read: Some(vec![absolute_path("/tmp/read-only").into()]),
                write: Some(vec![absolute_path("/tmp/write").into()]),
                glob_scan_max_depth: None,
                entries: None,
            }),
        };
        let request = CoreRequestPermissionProfile::try_from(request)
            .expect("API paths should convert to native paths");

        assert_eq!(
            granted_permission_profile_from_request(request),
            GrantedPermissionProfile {
                network: Some(AdditionalNetworkPermissions {
                    enabled: Some(true),
                }),
                file_system: Some(AdditionalFileSystemPermissions {
                    read: Some(vec![absolute_path("/tmp/read-only").into()]),
                    write: Some(vec![absolute_path("/tmp/write").into()]),
                    glob_scan_max_depth: None,
                    entries: Some(vec![
                        FileSystemSandboxEntry {
                            path: FileSystemPath::Path {
                                path: absolute_path("/tmp/read-only").into(),
                            },
                            access: FileSystemAccessMode::Read,
                        },
                        FileSystemSandboxEntry {
                            path: FileSystemPath::Path {
                                path: absolute_path("/tmp/write").into(),
                            },
                            access: FileSystemAccessMode::Write,
                        },
                    ]),
                }),
            }
        );
    }

    #[test]
    fn converts_request_permissions_into_canonical_granted_permissions() {
        let request = RequestPermissionProfile {
            network: None,
            file_system: Some(AdditionalFileSystemPermissions {
                read: None,
                write: None,
                glob_scan_max_depth: None,
                entries: Some(vec![FileSystemSandboxEntry {
                    path: FileSystemPath::Special {
                        value: FileSystemSpecialPath::Root,
                    },
                    access: FileSystemAccessMode::Write,
                }]),
            }),
        };
        let request = CoreRequestPermissionProfile::try_from(request)
            .expect("API paths should convert to native paths");

        assert_eq!(
            granted_permission_profile_from_request(request),
            GrantedPermissionProfile {
                network: None,
                file_system: Some(AdditionalFileSystemPermissions {
                    read: None,
                    write: None,
                    glob_scan_max_depth: None,
                    entries: Some(vec![FileSystemSandboxEntry {
                        path: FileSystemPath::Special {
                            value: FileSystemSpecialPath::Root,
                        },
                        access: FileSystemAccessMode::Write,
                    }]),
                }),
            }
        );
    }
}
