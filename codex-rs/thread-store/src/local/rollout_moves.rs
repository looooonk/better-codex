use std::collections::HashSet;
use std::path::PathBuf;

use crate::ThreadStoreError;
use crate::ThreadStoreResult;

pub(super) struct PendingRolloutMoves {
    moved: Vec<(PathBuf, PathBuf)>,
    operation: &'static str,
    committed: bool,
}

pub(super) fn move_rollouts(
    mut moves: Vec<(PathBuf, PathBuf)>,
    operation: &'static str,
) -> ThreadStoreResult<PendingRolloutMoves> {
    moves.sort_by(|(left, _), (right, _)| left.cmp(right));
    moves.dedup();
    let mut sources = HashSet::with_capacity(moves.len());
    let mut destinations = HashSet::with_capacity(moves.len());
    for (source, destination) in &moves {
        if !sources.insert(source.clone()) || !destinations.insert(destination.clone()) {
            return Err(move_error(operation, "rollout move paths are not unique"));
        }
        if !source
            .try_exists()
            .map_err(|err| move_error(operation, err))?
        {
            return Err(move_error(
                operation,
                format!("rollout `{}` does not exist", source.display()),
            ));
        }
        if destination
            .try_exists()
            .map_err(|err| move_error(operation, err))?
        {
            return Err(move_error(
                operation,
                format!("destination `{}` already exists", destination.display()),
            ));
        }
        let parent = destination.parent().ok_or_else(|| {
            move_error(
                operation,
                format!("destination `{}` has no parent", destination.display()),
            )
        })?;
        std::fs::create_dir_all(parent).map_err(|err| move_error(operation, err))?;
    }

    let mut pending = PendingRolloutMoves {
        moved: Vec::with_capacity(moves.len()),
        operation,
        committed: false,
    };
    for (source, destination) in moves {
        if let Err(err) = std::fs::rename(&source, &destination) {
            return Err(pending.fail(format!(
                "failed to {operation} thread: could not move `{}`: {err}",
                source.display()
            )));
        }
        pending.moved.push((source, destination));
    }
    Ok(pending)
}

impl PendingRolloutMoves {
    pub(super) fn commit(mut self) {
        self.committed = true;
    }

    pub(super) fn fail(mut self, cause: impl std::fmt::Display) -> ThreadStoreError {
        let cause = cause.to_string();
        let rollback_errors = self.rollback();
        self.committed = true;
        if rollback_errors.is_empty() {
            ThreadStoreError::Internal { message: cause }
        } else {
            ThreadStoreError::Internal {
                message: format!(
                    "{cause}; failed to restore rollouts after {}: {}",
                    self.operation,
                    rollback_errors.join("; ")
                ),
            }
        }
    }

    fn rollback(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        for (source, destination) in self.moved.drain(..).rev() {
            if let Err(err) = std::fs::rename(&destination, &source) {
                errors.push(format!(
                    "`{}` to `{}`: {err}",
                    destination.display(),
                    source.display()
                ));
            }
        }
        errors
    }
}

impl Drop for PendingRolloutMoves {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.rollback();
        }
    }
}

fn move_error(operation: &str, cause: impl std::fmt::Display) -> ThreadStoreError {
    ThreadStoreError::Internal {
        message: format!("failed to {operation} thread: {cause}"),
    }
}
