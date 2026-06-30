//! Shared state for app-level external editor launch coordination.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum ExternalEditorState {
    #[default]
    Closed,
    Requested,
    Active,
}
