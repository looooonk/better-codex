//! Client-local transcript math. Source in sessions and model requests stays ordinary Markdown.
mod layout;
mod raster;
mod terminal;
mod worker;

pub(crate) use layout::MathRun;
pub(crate) use layout::Measurements;
pub(crate) use layout::copy_source;
pub(crate) use raster::CellSize;
pub(crate) use raster::Formula;
pub(crate) use terminal::Graphics;

#[cfg(test)]
#[path = "equations/tests.rs"]
mod tests;
