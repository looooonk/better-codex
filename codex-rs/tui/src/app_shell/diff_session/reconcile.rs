use super::*;

pub(super) enum HistoryMatch {
    None,
    Unique(usize),
    Ambiguous,
}

pub(super) fn matching_history(histories: &[SessionFileHistory], file: &DiffFile) -> HistoryMatch {
    let source = file.old_path().or_else(|| file.new_path());
    let mut current_matches = histories.iter().enumerate().filter_map(|(index, history)| {
        equivalent_paths(history.current_path.as_ref(), source).then_some(index)
    });
    match (current_matches.next(), current_matches.next()) {
        (Some(index), None) => return HistoryMatch::Unique(index),
        (Some(_), Some(_)) => return HistoryMatch::Ambiguous,
        (None, None) => {}
        (None, Some(_)) => unreachable!("a second history match requires a first match"),
    }
    if file.kind() != DiffFileKind::Added {
        return HistoryMatch::None;
    }
    let mut extinct_original_matches =
        histories.iter().enumerate().filter_map(|(index, history)| {
            (!history.current_exists && equivalent_paths(history.original_path.as_ref(), source))
                .then_some(index)
        });
    match (
        extinct_original_matches.next(),
        extinct_original_matches.next(),
    ) {
        (None, None) => HistoryMatch::None,
        (Some(index), None) => HistoryMatch::Unique(index),
        (Some(_), Some(_)) => HistoryMatch::Ambiguous,
        (None, Some(_)) => unreachable!("a second history match requires a first match"),
    }
}

pub(super) fn has_possible_namespace_transition(
    histories: &[SessionFileHistory],
    file: &DiffFile,
) -> bool {
    let Some(source) = file.old_path().or_else(|| file.new_path()) else {
        return false;
    };
    histories.iter().any(|history| {
        [&history.original_path, &history.current_path]
            .into_iter()
            .flatten()
            .any(|path| path.possible_namespace_variant(source))
    })
}

pub(super) fn reconnect_extinct_originals(
    histories: &mut Vec<SessionFileHistory>,
    budget: &mut CompositionBudget,
) -> bool {
    let mut truncated = false;
    loop {
        let mut reconnect = None;
        for (extinct_index, extinct) in histories
            .iter()
            .enumerate()
            .filter(|(_, history)| history.original_exists && !history.current_exists)
        {
            let Some(original_path) = &extinct.original_path else {
                continue;
            };
            let recreated = histories
                .iter()
                .enumerate()
                .filter(|(_, history)| !history.original_exists && history.current_exists)
                .filter(|(_, history)| {
                    history
                        .current_path
                        .as_ref()
                        .is_some_and(|path| path.equivalent(original_path))
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if recreated.len() > 1 {
                truncated = true;
                continue;
            }
            let Some(&recreated_index) = recreated.first() else {
                continue;
            };
            let matching_extinct = histories
                .iter()
                .filter(|history| history.original_exists && !history.current_exists)
                .filter(|history| {
                    history
                        .original_path
                        .as_ref()
                        .is_some_and(|path| path.equivalent(original_path))
                })
                .count();
            if matching_extinct == 1 {
                reconnect = Some((extinct_index, recreated_index));
                break;
            }
            truncated = true;
        }
        let Some((extinct_index, recreated_index)) = reconnect else {
            break;
        };
        let recreated = histories.remove(recreated_index);
        let extinct_index = extinct_index - usize::from(recreated_index < extinct_index);
        if let Err(recreated) = histories[extinct_index].reconnect_recreated(recreated, budget) {
            histories.insert(recreated_index, *recreated);
            truncated = true;
            break;
        }
    }
    truncated || has_unresolved_ambiguity(histories)
}

impl SessionFileHistory {
    fn reconnect_recreated(
        &mut self,
        mut recreated: Self,
        budget: &mut CompositionBudget,
    ) -> Result<(), Box<Self>> {
        if !self.composable
            || !recreated.composable
            || !self.current.is_empty()
            || recreated.baseline.iter().any(Option::is_some)
        {
            return Err(Box::new(recreated));
        }
        let line_offset = self.line_offset.min(recreated.line_offset);
        let baseline_prefix = self.line_offset - line_offset;
        let current_prefix = recreated.line_offset - line_offset;
        let Some(required_slots) = baseline_prefix.checked_add(current_prefix) else {
            return Err(Box::new(recreated));
        };
        if baseline_prefix.saturating_add(self.baseline.len()) > MAX_SESSION_COMPOSE_LINES
            || current_prefix.saturating_add(recreated.current.len()) > MAX_SESSION_COMPOSE_LINES
            || !budget.reserve(required_slots)
        {
            return Err(Box::new(recreated));
        }
        self.baseline
            .splice(0..0, std::iter::repeat_n(None, baseline_prefix));
        for line in &mut recreated.current {
            line.origin = None;
        }
        recreated.current.splice(
            0..0,
            std::iter::repeat_with(|| TrackedLine {
                origin: None,
                text: None,
            })
            .take(current_prefix),
        );
        self.current = recreated.current;
        self.current_path = recreated.current_path;
        self.current_exists = true;
        self.current_incarnation_was_added = recreated.current_incarnation_was_added;
        self.status = recreated.status;
        self.latest_file = recreated.latest_file;
        self.metadata.reconnect(recreated.metadata);
        self.applied_files = self.applied_files.saturating_add(recreated.applied_files);
        self.line_offset = line_offset;
        Ok(())
    }
}

fn has_unresolved_ambiguity(histories: &[SessionFileHistory]) -> bool {
    histories.iter().enumerate().any(|(index, history)| {
        if !history.current_exists {
            return false;
        }
        let Some(current_path) = &history.current_path else {
            return false;
        };
        histories.iter().enumerate().any(|(other_index, other)| {
            if index == other_index {
                return false;
            }
            let duplicate_current = other.current_exists
                && other
                    .current_path
                    .as_ref()
                    .is_some_and(|path| path.equivalent(current_path));
            let overwritten_extinct_original = history.original_exists
                && other.original_exists
                && !other.current_exists
                && other
                    .original_path
                    .as_ref()
                    .is_some_and(|path| path.equivalent(current_path));
            duplicate_current || overwritten_extinct_original
        })
    }) || has_live_rename_cycle(histories)
}

fn has_live_rename_cycle(histories: &[SessionFileHistory]) -> bool {
    (0..histories.len()).any(|start| {
        let mut visited = vec![false; histories.len()];
        let mut current = start;
        loop {
            if visited[current] {
                return true;
            }
            visited[current] = true;
            let history = &histories[current];
            if !history.original_exists || !history.current_exists {
                return false;
            }
            let Some(current_path) = &history.current_path else {
                return false;
            };
            let mut destinations = histories
                .iter()
                .enumerate()
                .filter_map(|(index, candidate)| {
                    (candidate.original_exists
                        && candidate.current_exists
                        && candidate
                            .original_path
                            .as_ref()
                            .is_some_and(|path| path.equivalent(current_path)))
                    .then_some(index)
                });
            let Some(next) = destinations.next() else {
                return false;
            };
            if destinations.next().is_some() || next == current {
                return false;
            }
            current = next;
        }
    })
}

fn equivalent_paths(left: Option<&DiffPath>, right: Option<&DiffPath>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.equivalent(right),
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}
