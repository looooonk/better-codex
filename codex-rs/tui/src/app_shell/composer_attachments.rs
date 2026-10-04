use super::super::attachments::ImageAttachment;
use super::ComposerState;
use codex_app_server_protocol::ImageReference;
use codex_app_server_protocol::UserInput;

impl ComposerState {
    pub(in crate::app_shell) fn has_images(&self) -> bool {
        !self.images.is_empty()
    }

    pub(in crate::app_shell) fn image_count(&self) -> usize {
        self.images.len()
    }

    pub(in crate::app_shell) fn image_bytes(&self) -> usize {
        self.images.iter().map(|image| image.url.len()).sum()
    }

    pub(in crate::app_shell) fn image_summary(&self) -> String {
        self.images
            .iter()
            .enumerate()
            .map(|(index, image)| format!("[{}: {}]", index + 1, image.label))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub(in crate::app_shell) fn attach_images(&mut self, images: Vec<ImageAttachment>) {
        self.images.extend(images);
    }

    pub(in crate::app_shell) fn clear_images(&mut self) {
        self.images.clear();
    }

    pub(in crate::app_shell) fn remove_image(&mut self, index: usize) -> bool {
        if index >= self.images.len() {
            return false;
        }
        self.images.remove(index);
        true
    }

    pub(in crate::app_shell) fn submission_items(&self, prompt: &str) -> Vec<UserInput> {
        let mut items = Vec::new();
        if !prompt.trim().is_empty() {
            items.push(UserInput::Text {
                text: prompt.to_string(),
                text_elements: Vec::new(),
            });
        }
        items.extend(self.images.iter().map(|image| UserInput::Image {
            image: ImageReference::Inline {
                url: image.url.to_string(),
            },
            detail: None,
        }));
        items
    }

    pub(in crate::app_shell) fn restore_input_images(&mut self, items: &[UserInput]) {
        let images = items
            .iter()
            .filter_map(|item| match item {
                UserInput::Image {
                    image: ImageReference::Inline { url },
                    ..
                } => Some(ImageAttachment {
                    label: "restored image".to_string(),
                    url: url.clone().into(),
                }),
                _ => None,
            })
            .collect::<Vec<_>>();
        self.restore_queued_images(images);
    }

    pub(in crate::app_shell) fn restore_queued_images(&mut self, images: Vec<ImageAttachment>) {
        let destination = self
            .queued_index
            .and(self.draft_before_queue.as_mut())
            .map_or(&mut self.images, |draft| &mut draft.images);
        destination.splice(..0, images);
    }
}
