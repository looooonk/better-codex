//! Shared thread replay modes used when restoring app-server history into UI state.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum ReplayKind {
    ResumeInitialMessages,
    ThreadSnapshot,
}
