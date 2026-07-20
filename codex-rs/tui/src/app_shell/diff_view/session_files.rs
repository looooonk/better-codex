use super::DiffStore;
use crate::app_shell::diff_session::ComposedSessionFiles;
use crate::app_shell::diff_session::compose_session_files;

struct RetainedSessionFiles {
    files: Vec<super::DiffFile>,
    truncated: bool,
}

impl DiffStore {
    pub(in crate::app_shell) fn has_recorded_history(&self) -> bool {
        !self.turns.is_empty()
    }

    pub(super) fn refresh_session_files(&mut self) {
        let RetainedSessionFiles {
            files,
            truncated: retained_sources_truncated,
        } = self.retained_session_files();
        let ComposedSessionFiles { files, truncated } = compose_session_files(files);
        self.session_files = files;
        self.retained_sources_truncated = retained_sources_truncated;
        self.composition_truncated = truncated;
    }

    fn retained_session_files(&self) -> RetainedSessionFiles {
        let mut retained = Vec::new();
        let mut truncated = false;
        for turn in &self.turns {
            let has_uncertain_terminal_item = turn
                .items
                .iter()
                .any(|item| !item.status.is_session_edit() && !item.files.is_empty());
            let items = turn
                .items
                .iter()
                .filter(|item| item.status.is_session_edit())
                .flat_map(|item| {
                    item.files.iter().enumerate().map(move |(index, file)| {
                        (file, index < item.complete_files, item.revision)
                    })
                })
                .collect::<Vec<_>>();
            let Some(aggregate) = &turn.aggregate else {
                truncated |= has_uncertain_terminal_item
                    || turn
                        .items
                        .iter()
                        .any(|item| item.status.is_session_edit() && item.truncated);
                retained.extend(items.into_iter().map(|(file, _, _)| file.clone()));
                continue;
            };
            if !aggregate.truncated {
                if !aggregate.files.is_empty() {
                    truncated |= turn.items.iter().any(|item| {
                        if item.revision <= turn.aggregate_item_revision {
                            item.status.is_session_edit() && item.omitted_files
                        } else {
                            !item.status.is_session_edit() && !item.files.is_empty()
                                || item.status.is_session_edit() && item.truncated
                        }
                    });
                    retained.extend(aggregate.files.iter().cloned());
                    retained.extend(
                        items
                            .iter()
                            .filter(|(_, _, revision)| *revision <= turn.aggregate_item_revision)
                            .map(|(file, _, _)| *file)
                            .filter(|file| {
                                let stats = file.stats();
                                file.kind() == super::DiffFileKind::Renamed
                                    && stats.additions == 0
                                    && stats.removals == 0
                                    && !aggregate
                                        .files
                                        .iter()
                                        .any(|aggregate_file| aggregate_file.overlaps(file))
                            })
                            .cloned(),
                    );
                    retained.extend(
                        items
                            .into_iter()
                            .filter(|(_, _, revision)| *revision > turn.aggregate_item_revision)
                            .map(|(file, _, _)| file.clone()),
                    );
                    continue;
                }

                let items_truncated = turn
                    .items
                    .iter()
                    .any(|item| item.status.is_session_edit() && item.truncated);
                let ComposedSessionFiles {
                    files,
                    truncated: composition_truncated,
                } = compose_session_files(items.into_iter().map(|(file, _, _)| file.clone()));
                truncated |= has_uncertain_terminal_item
                    || items_truncated
                    || composition_truncated
                    || !files.is_empty();
                retained.extend(files);
                continue;
            }

            truncated = true;

            let complete_items = items
                .iter()
                .filter_map(|(file, complete, _)| complete.then_some(*file))
                .collect::<Vec<_>>();
            retained.extend(complete_items.iter().copied().cloned());
            retained.extend(
                aggregate
                    .files
                    .iter()
                    .filter(|file| {
                        !complete_items
                            .iter()
                            .any(|item_file| item_file.overlaps(file))
                    })
                    .cloned(),
            );
            retained.extend(
                items
                    .into_iter()
                    .filter_map(|(file, complete, _)| (!complete).then_some(file))
                    .filter(|file| {
                        !aggregate
                            .files
                            .iter()
                            .any(|aggregate_file| aggregate_file.overlaps(file))
                    })
                    .cloned(),
            );
        }
        RetainedSessionFiles {
            files: retained,
            truncated,
        }
    }
}
