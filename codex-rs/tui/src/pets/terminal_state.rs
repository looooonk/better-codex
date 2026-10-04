use super::AmbientPetDraw;
use super::PetImageRenderError;
use super::PetImageRenderState;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

static ACTIVE: Mutex<Option<Weak<Mutex<PetImageRenderState>>>> = Mutex::new(None);

#[derive(Default)]
pub(crate) struct TerminalPetImage(Arc<Mutex<PetImageRenderState>>);

impl TerminalPetImage {
    pub(crate) fn draw(
        &self,
        writer: &mut impl Write,
        request: Option<AmbientPetDraw>,
    ) -> Result<(), PetImageRenderError> {
        if request.is_some() {
            *ACTIVE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::downgrade(&self.0));
        }
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        super::render_ambient_pet_image(writer, &mut state, request)
    }
}

pub(crate) fn clear_active_terminal_image(writer: &mut impl Write) -> std::io::Result<()> {
    let active = ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(Weak::upgrade);
    if let Some(active) = active
        && let Ok(mut state) = active.try_lock()
    {
        super::render_ambient_pet_image(writer, &mut state, /*request*/ None)
            .map_err(std::io::Error::other)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "terminal_state_tests.rs"]
mod tests;
