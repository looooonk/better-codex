use super::DiffStore;
use crate::app_shell::diff_session::compose_session_files;

impl DiffStore {
    pub(super) fn refresh_session_files(&mut self) {
        self.session_files =
            compose_session_files(self.retained_session_file_refs().cloned().collect());
    }

    fn retained_session_file_refs(&self) -> impl Iterator<Item = &super::DiffFile> {
        self.turns.iter().flat_map(|turn| {
            turn.aggregate
                .iter()
                .flat_map(|stored| &stored.files)
                .chain(
                    turn.items
                        .iter()
                        .filter(|item| item.status.is_session_edit())
                        .flat_map(|item| item.files.iter())
                        .filter(move |file| {
                            !turn.aggregate.as_ref().is_some_and(|aggregate| {
                                aggregate.files.iter().any(|aggregate_file| {
                                    [file.old_label(), file.new_label()]
                                        .into_iter()
                                        .flatten()
                                        .any(|path| {
                                            aggregate_file.old_label() == Some(path)
                                                || aggregate_file.new_label() == Some(path)
                                        })
                                })
                            })
                        }),
                )
        })
    }
}
