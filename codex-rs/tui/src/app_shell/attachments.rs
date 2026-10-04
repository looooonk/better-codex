use super::ShellState;
use codex_protocol::openai_models::InputModality;
use codex_utils_image::PromptImageMode;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::bail;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

const MAX_IMAGES: usize = 10;
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const MAX_ENCODED_IMAGES_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ImageAttachment {
    pub(super) label: String,
    pub(super) url: Arc<str>,
}

fn load_image(path: &Path, bytes: Vec<u8>) -> Result<ImageAttachment> {
    if bytes.len() > MAX_IMAGE_BYTES {
        bail!(
            "image exceeds the 20 MiB attachment limit: {}",
            path.display()
        );
    }
    let image = codex_utils_image::load_for_prompt_bytes(path, bytes, PromptImageMode::Original)?;
    if image.bytes.len() > MAX_IMAGE_BYTES {
        bail!(
            "encoded image exceeds the 20 MiB attachment limit: {}",
            path.display()
        );
    }
    Ok(ImageAttachment {
        label: path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        url: image.into_data_url().into(),
    })
}

impl ShellState {
    pub(super) async fn attach_image_paths(&mut self, paths: Vec<PathBuf>) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }
        if self.composer.image_count().saturating_add(paths.len()) > MAX_IMAGES {
            bail!("a message can contain at most {MAX_IMAGES} images");
        }
        let images = tokio::task::spawn_blocking(move || {
            paths
                .into_iter()
                .map(|path| {
                    let mut bytes = Vec::new();
                    std::fs::File::open(&path)
                        .wrap_err_with(|| format!("cannot open image {}", path.display()))?
                        .take((MAX_IMAGE_BYTES + 1) as u64)
                        .read_to_end(&mut bytes)?;
                    load_image(&path, bytes)
                })
                .collect::<Result<Vec<_>>>()
        })
        .await
        .wrap_err("image loading failed")??;
        if self.composer.image_bytes() + images.iter().map(|image| image.url.len()).sum::<usize>()
            > MAX_ENCODED_IMAGES_BYTES
        {
            bail!("images in one message must fit within 64 MiB after encoding");
        }
        self.composer.attach_images(images);
        Ok(())
    }

    pub(super) async fn run_attach_command(&mut self, args: &str) -> Result<()> {
        if args.is_empty() && self.composer.has_images() {
            self.push_system(self.composer.image_summary());
            return Ok(());
        }
        let paths = shlex::split(args).filter(|paths| !paths.is_empty());
        let Some(paths) = paths else {
            self.push_system(
                "usage: /attach <local-image> [local-image ...]; quote paths containing spaces",
            );
            return Ok(());
        };
        if self.composer.queued_edit_position().is_some() {
            self.push_error("finish editing the queued message before attaching images");
            return Ok(());
        }
        let paths = paths.into_iter().map(PathBuf::from).collect();
        match self.attach_image_paths(paths).await {
            Ok(()) => self.push_status(self.composer.image_summary()),
            Err(error) => self.push_error(error.to_string()),
        }
        Ok(())
    }

    pub(super) fn run_detach_command(&mut self, args: &str) -> Result<()> {
        if args.is_empty() || args == "all" {
            self.composer.clear_images();
            self.push_status("images removed from draft");
        } else if let Ok(index) = args.parse::<usize>() {
            if index == 0 || !self.composer.remove_image(index.saturating_sub(1)) {
                self.push_error("image number is outside the attachment list");
            }
        } else {
            self.push_error("usage: /detach [number|all]");
        }
        Ok(())
    }

    pub(super) async fn paste_image(&mut self) {
        if self.composer.queued_edit_position().is_some()
            || self.composer.image_count() >= MAX_IMAGES
        {
            self.push_error(
                "finish the queued edit or remove an attachment before pasting an image",
            );
            return;
        }
        let image = tokio::task::spawn_blocking(|| {
            let (bytes, _) = crate::clipboard_paste::paste_image_as_png()?;
            load_image(Path::new("clipboard.png"), bytes)
        })
        .await;
        match image {
            Ok(Ok(image))
                if self.composer.image_bytes() + image.url.len() > MAX_ENCODED_IMAGES_BYTES =>
            {
                self.push_error("images in one message must fit within 64 MiB after encoding");
            }
            Ok(Ok(image)) => {
                self.composer.attach_images(vec![image]);
                self.push_status(self.composer.image_summary());
            }
            Ok(Err(error)) => self.push_error(error.to_string()),
            Err(error) => self.push_error(format!("clipboard image could not be read: {error}")),
        }
    }

    pub(super) fn reject_unsupported_images(&mut self) -> bool {
        if self.composer.image_count() > MAX_IMAGES
            || self.composer.image_bytes() > MAX_ENCODED_IMAGES_BYTES
        {
            self.push_error("draft images exceed the attachment limit; use /detach to remove images before sending");
            return true;
        }
        if self.composer.has_images()
            && self
                .available_models
                .iter()
                .find(|model| model.model == self.model)
                .is_some_and(|model| !model.input_modalities.contains(&InputModality::Image))
        {
            self.push_error("this model does not accept images; switch models or use /detach");
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
#[path = "attachments_tests.rs"]
mod tests;
