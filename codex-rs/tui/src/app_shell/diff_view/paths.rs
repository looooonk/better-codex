use super::DiffRoot;
use super::DiffRootProvenance;
use super::DiffStore;
use std::path::Path;

impl DiffStore {
    pub(in crate::app_shell) fn with_display_root(root: &Path) -> Self {
        Self {
            display_root: Some(DiffRoot {
                path: root.to_path_buf(),
                provenance: DiffRootProvenance::CwdFallback { generation: 0 },
            }),
            item_root: Some(root.to_path_buf()),
            ..Self::default()
        }
    }

    pub(in crate::app_shell) fn set_display_root(&mut self, root: &Path) {
        if self.item_root.as_deref() == Some(root) && self.display_root.is_some() {
            return;
        }
        self.cwd_generation = self.cwd_generation.wrapping_add(1);
        self.display_root = Some(DiffRoot {
            path: root.to_path_buf(),
            provenance: DiffRootProvenance::CwdFallback {
                generation: self.cwd_generation,
            },
        });
        self.item_root = Some(root.to_path_buf());
        for turn in &mut self.turns {
            if let Some(aggregate) = &mut turn.aggregate {
                for file in &mut aggregate.files {
                    file.rebase_display_root(root);
                }
            }
            for item in &mut turn.items {
                for file in &mut item.files {
                    file.rebase_display_root(root);
                }
            }
        }
        self.refresh_session_files();
    }

    pub(in crate::app_shell) fn set_git_root(&mut self, root: &Path) {
        self.display_root = Some(DiffRoot {
            path: root.to_path_buf(),
            provenance: DiffRootProvenance::ConfirmedGit,
        });
        for turn in &mut self.turns {
            if let Some(aggregate) = &mut turn.aggregate {
                if aggregate.path_root.as_ref().is_some_and(|path_root| {
                    matches!(
                        path_root.provenance,
                        DiffRootProvenance::CwdFallback { generation }
                            if generation == self.cwd_generation
                    )
                }) {
                    for file in &mut aggregate.files {
                        file.reroot_paths(root, root);
                    }
                    aggregate.path_root = Some(DiffRoot {
                        path: root.to_path_buf(),
                        provenance: DiffRootProvenance::ConfirmedGit,
                    });
                } else {
                    for file in &mut aggregate.files {
                        file.rebase_display_root(root);
                    }
                }
            }
            for item in &mut turn.items {
                for file in &mut item.files {
                    file.rebase_display_root(root);
                }
            }
        }
        self.refresh_session_files();
    }

    pub(in crate::app_shell) fn confirm_no_git_root(&mut self) {
        let Some(root) = self.item_root.clone() else {
            self.display_root = None;
            return;
        };
        for turn in &mut self.turns {
            if let Some(aggregate) = &mut turn.aggregate {
                if aggregate.path_root.as_ref().is_some_and(|path_root| {
                    matches!(
                        path_root.provenance,
                        DiffRootProvenance::CwdFallback { generation }
                            if generation == self.cwd_generation
                    )
                }) {
                    aggregate.path_root = Some(DiffRoot {
                        path: root.clone(),
                        provenance: DiffRootProvenance::ConfirmedCwd,
                    });
                }
                for file in &mut aggregate.files {
                    file.rebase_display_root(&root);
                }
            }
            for item in &mut turn.items {
                for file in &mut item.files {
                    file.rebase_display_root(&root);
                }
            }
        }
        self.cwd_generation = self.cwd_generation.wrapping_add(1);
        self.display_root = Some(DiffRoot {
            path: root,
            provenance: DiffRootProvenance::CwdFallback {
                generation: self.cwd_generation,
            },
        });
        self.refresh_session_files();
    }
}
