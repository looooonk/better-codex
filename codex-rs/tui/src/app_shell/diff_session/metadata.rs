use super::super::diff_model::DiffFile;
use super::super::diff_model::DiffMetadata;

#[derive(Default)]
pub(super) struct SessionMetadata {
    mode: TransitionState,
    binary_oid: TransitionState,
    unknown: bool,
}

impl SessionMetadata {
    pub(super) fn apply(&mut self, file: &DiffFile) {
        let metadata = file.metadata();
        if let Some((old, new)) = metadata.mode_transition() {
            self.mode.apply(old, new);
        }
        if let Some((old, new)) = metadata.binary_oid_transition() {
            self.binary_oid.apply(old, new);
        }
        self.unknown |= metadata.is_unknown();
    }

    pub(super) fn changed(&self) -> bool {
        self.unknown || self.mode.changed() || self.binary_oid.changed()
    }

    pub(super) fn is_exact(&self) -> bool {
        !self.unknown && !self.mode.uncertain && !self.binary_oid.uncertain
    }

    pub(super) fn reconnect(&mut self, newer: Self) {
        self.mode.reconnect(newer.mode);
        self.binary_oid.reconnect(newer.binary_oid);
        self.unknown |= newer.unknown;
    }

    pub(super) fn finish(self) -> DiffMetadata {
        let unknown = self.unknown || self.mode.uncertain || self.binary_oid.uncertain;
        let metadata = DiffMetadata::composed(self.mode.finish(), self.binary_oid.finish());
        if unknown {
            metadata.with_unknown()
        } else {
            metadata
        }
    }
}

#[derive(Default)]
struct TransitionState {
    original: Option<Option<String>>,
    current: Option<Option<String>>,
    uncertain: bool,
}

impl TransitionState {
    fn apply(&mut self, old: Option<&str>, new: Option<&str>) {
        match (&self.original, &self.current) {
            (None, None) => {
                self.original = Some(old.map(str::to_string));
                self.current = Some(new.map(str::to_string));
            }
            (Some(_), Some(current)) if current.as_deref() == old => {
                self.current = Some(new.map(str::to_string));
            }
            (Some(original), Some(current))
                if original.as_deref() == old && current.as_deref() == new => {}
            (Some(_), Some(_)) => self.uncertain = true,
            (Some(_), None) | (None, Some(_)) => self.uncertain = true,
        }
    }

    fn changed(&self) -> bool {
        self.uncertain || self.original != self.current
    }

    fn reconnect(&mut self, newer: Self) {
        self.uncertain |= newer.uncertain;
        match (newer.original, newer.current) {
            (None, None) => {}
            (Some(original), Some(current)) => {
                self.apply(original.as_deref(), current.as_deref());
            }
            (Some(_), None) | (None, Some(_)) => self.uncertain = true,
        }
    }

    fn finish(self) -> Option<(Option<String>, Option<String>)> {
        if self.uncertain {
            return None;
        }
        self.original
            .zip(self.current)
            .filter(|(old, new)| old != new)
    }
}
