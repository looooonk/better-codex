//! Shared display mode for interrupted-turn restoration notices.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum InterruptedTurnNoticeMode {
    #[default]
    Default,
    Suppress,
}
