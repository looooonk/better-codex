//! Amazon Bedrock credential discovery, setup, and post-login checks within authentication.

use crate::key_hint::KeyBindingListExt;
use crate::wrapping::word_wrap_lines;
use codex_app_server_protocol::AwsCredentialType;
use codex_app_server_protocol::BedrockAwsProfile;
use codex_app_server_protocol::BedrockCheckGovCloudRequirementsParams;
use codex_app_server_protocol::BedrockCheckGovCloudRequirementsResponse;
use codex_app_server_protocol::BedrockDiscoverParams;
use codex_app_server_protocol::BedrockDiscoverResponse;
use codex_app_server_protocol::BedrockEnvironmentCredential;
use codex_app_server_protocol::BedrockSetupParams;
use codex_app_server_protocol::BedrockSetupResponse;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::LoginAccountParams;
use codex_app_server_protocol::LoginAccountResponse;
use codex_app_server_protocol::RequestId;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::prelude::Widget;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

const GOV_CLOUD_GUIDANCE_URL: &str =
    "https://learn.chatgpt.com/docs/enterprise/govcloud-configuration";

#[derive(Clone)]
pub(super) struct BedrockState {
    view: BedrockView,
    highlighted: usize,
    profiles: Vec<BedrockAwsProfile>,
    environment_credentials: Vec<BedrockEnvironmentCredential>,
}

#[derive(Clone)]
enum BedrockView {
    Discovering(RequestId),
    Methods(BedrockMethodList),
    ProfileEntry(String),
    AccessKeyEntry {
        values: [String; 3],
        selected_field: usize,
    },
    ApiKeyEntry(String),
    RegionEntry {
        credential: BedrockCredential,
        value: String,
    },
    EnvironmentInstructions,
    Configuring(RequestId),
    CheckingGovCloud(RequestId),
    // Render-clamped scroll offset shared with the auth widget's cloned state.
    GovCloudWarning(Arc<AtomicUsize>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BedrockMethodList {
    Detected,
    All,
}

#[derive(Clone)]
enum BedrockCredential {
    Profile(String),
    Environment,
    AccessKeys {
        access_key_id: String,
        secret_access_key: String,
        session_token: Option<String>,
    },
    ApiKey(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BedrockMethod {
    Profile(usize),
    Environment,
    OtherMethods,
    ManualProfile,
    AccessKeys,
    EnvironmentInstructions,
    ApiKey,
}

enum BedrockAction {
    BackToAuth,
    Configure(BedrockCredential, String),
    ContinueAfterGovCloudWarning,
}

impl BedrockState {
    fn discovering(request_id: RequestId) -> Self {
        Self {
            view: BedrockView::Discovering(request_id),
            highlighted: 0,
            profiles: Vec::new(),
            environment_credentials: Vec::new(),
        }
    }

    fn discovered(response: BedrockDiscoverResponse) -> Self {
        Self {
            view: BedrockView::Methods(BedrockMethodList::Detected),
            highlighted: 0,
            profiles: response.profiles,
            environment_credentials: response.environment_credentials,
        }
    }

    fn methods(&self) -> Vec<BedrockMethod> {
        if matches!(self.view, BedrockView::EnvironmentInstructions) {
            return vec![BedrockMethod::OtherMethods];
        }
        let detected = matches!(self.view, BedrockView::Methods(BedrockMethodList::Detected));
        if detected && !self.profiles.is_empty() {
            let mut methods = (0..self.profiles.len())
                .map(BedrockMethod::Profile)
                .collect::<Vec<_>>();
            if !self.environment_credentials.is_empty() {
                methods.push(BedrockMethod::Environment);
            }
            methods.extend([BedrockMethod::OtherMethods, BedrockMethod::ApiKey]);
            return methods;
        }
        if detected && !self.environment_credentials.is_empty() {
            return vec![
                BedrockMethod::Environment,
                BedrockMethod::OtherMethods,
                BedrockMethod::ApiKey,
            ];
        }
        let mut methods = vec![
            BedrockMethod::ManualProfile,
            BedrockMethod::AccessKeys,
            BedrockMethod::EnvironmentInstructions,
        ];
        if detected {
            methods.push(BedrockMethod::ApiKey);
        }
        methods
    }

    pub(super) fn is_text_entry_active(&self) -> bool {
        matches!(
            self.view,
            BedrockView::ProfileEntry(_)
                | BedrockView::AccessKeyEntry { .. }
                | BedrockView::ApiKeyEntry(_)
                | BedrockView::RegionEntry { .. }
        )
    }

    fn active_input_mut(&mut self) -> Option<&mut String> {
        match &mut self.view {
            BedrockView::ProfileEntry(value)
            | BedrockView::ApiKeyEntry(value)
            | BedrockView::RegionEntry { value, .. } => Some(value),
            BedrockView::AccessKeyEntry {
                values,
                selected_field,
            } => Some(&mut values[*selected_field]),
            _ => None,
        }
    }

    fn enter_region(&mut self, credential: BedrockCredential, value: String) {
        self.view = BedrockView::RegionEntry { credential, value };
    }

    fn select_method(&mut self, method: BedrockMethod) -> Option<BedrockAction> {
        self.highlighted = 0;
        match method {
            BedrockMethod::Profile(index) => {
                let profile = self.profiles[index].clone();
                let credential = BedrockCredential::Profile(profile.name);
                if let Some(region) = profile.region {
                    return Some(BedrockAction::Configure(credential, region));
                }
                self.enter_region(credential, String::new());
            }
            BedrockMethod::Environment => {
                let credential = BedrockCredential::Environment;
                if let Some(region) = self
                    .environment_credentials
                    .iter()
                    .find_map(|credential| credential.region.clone())
                {
                    return Some(BedrockAction::Configure(credential, region));
                }
                self.enter_region(credential, String::new());
            }
            BedrockMethod::OtherMethods => {
                self.view = BedrockView::Methods(BedrockMethodList::All);
            }
            BedrockMethod::ManualProfile => {
                self.view = BedrockView::ProfileEntry(String::new());
            }
            BedrockMethod::AccessKeys => {
                self.view = BedrockView::AccessKeyEntry {
                    values: Default::default(),
                    selected_field: 0,
                };
            }
            BedrockMethod::EnvironmentInstructions => {
                self.view = BedrockView::EnvironmentInstructions;
            }
            BedrockMethod::ApiKey => {
                self.view = BedrockView::ApiKeyEntry(String::new());
            }
        }
        None
    }

    fn handle_key_event(&mut self, key_event: &KeyEvent) -> Option<BedrockAction> {
        match &self.view {
            BedrockView::CheckingGovCloud(_) => return None,
            BedrockView::GovCloudWarning(scroll) => {
                let offset = scroll.load(Ordering::Relaxed);
                if keys::MOVE_UP.is_pressed(*key_event) {
                    scroll.store(offset.saturating_sub(1), Ordering::Relaxed);
                } else if keys::MOVE_DOWN.is_pressed(*key_event) {
                    scroll.store(offset.saturating_add(1), Ordering::Relaxed);
                }
                return (key_event.kind == KeyEventKind::Press
                    && keys::CONFIRM.is_pressed(*key_event))
                .then_some(BedrockAction::ContinueAfterGovCloudWarning);
            }
            _ => {}
        }
        if keys::CANCEL.is_pressed(*key_event) {
            let leave_wizard = matches!(
                self.view,
                BedrockView::Discovering(_) | BedrockView::Methods(BedrockMethodList::Detected)
            );
            if leave_wizard {
                return Some(BedrockAction::BackToAuth);
            }
            self.view = BedrockView::Methods(BedrockMethodList::Detected);
            self.highlighted = 0;
            return None;
        }
        if matches!(self.view, BedrockView::Configuring(_)) {
            return None;
        }

        if matches!(
            self.view,
            BedrockView::Methods(_) | BedrockView::EnvironmentInstructions
        ) {
            let methods = self.methods();
            if keys::MOVE_UP.is_pressed(*key_event) {
                self.highlighted = (self.highlighted + methods.len() - 1) % methods.len();
            } else if keys::MOVE_DOWN.is_pressed(*key_event) {
                self.highlighted = (self.highlighted + 1) % methods.len();
            } else if keys::CONFIRM.is_pressed(*key_event) {
                return self.select_method(methods[self.highlighted]);
            } else if let KeyCode::Char(digit @ '1'..='9') = key_event.code
                && let Some(method) = methods.get(digit as usize - '1' as usize).copied()
            {
                return self.select_method(method);
            }
            return None;
        }

        if let BedrockView::AccessKeyEntry {
            values,
            selected_field,
        } = &mut self.view
        {
            if key_event.code == KeyCode::Up || key_event.code == KeyCode::BackTab {
                *selected_field = (*selected_field + values.len() - 1) % values.len();
                return None;
            }
            if key_event.code == KeyCode::Down || key_event.code == KeyCode::Tab {
                *selected_field = (*selected_field + 1) % values.len();
                return None;
            }
        }

        if keys::CONFIRM.is_pressed(*key_event) {
            return self.confirm_input();
        }
        if let Some(input) = self.active_input_mut() {
            match key_event.code {
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(character)
                    if matches!(key_event.kind, KeyEventKind::Press | KeyEventKind::Repeat)
                        && !key_event.modifiers.intersects(
                            KeyModifiers::SUPER | KeyModifiers::CONTROL | KeyModifiers::ALT,
                        ) =>
                {
                    input.push(character);
                }
                _ => {}
            }
        }
        None
    }

    fn confirm_input(&mut self) -> Option<BedrockAction> {
        let credential = match &mut self.view {
            BedrockView::ProfileEntry(value) if !value.trim().is_empty() => {
                BedrockCredential::Profile(value.trim().to_string())
            }
            BedrockView::ApiKeyEntry(value) if !value.trim().is_empty() => {
                BedrockCredential::ApiKey(value.trim().to_string())
            }
            BedrockView::RegionEntry { credential, value } if !value.trim().is_empty() => {
                return Some(BedrockAction::Configure(
                    credential.clone(),
                    value.trim().to_string(),
                ));
            }
            BedrockView::AccessKeyEntry {
                values,
                selected_field,
            } => {
                if *selected_field < 2 {
                    if !values[*selected_field].trim().is_empty() {
                        *selected_field += 1;
                    }
                    return None;
                }
                if values[0].trim().is_empty() || values[1].trim().is_empty() {
                    *selected_field = if values[0].trim().is_empty() { 0 } else { 1 };
                    return None;
                }
                BedrockCredential::AccessKeys {
                    access_key_id: values[0].trim().to_string(),
                    secret_access_key: values[1].trim().to_string(),
                    session_token: (!values[2].trim().is_empty())
                        .then(|| values[2].trim().to_string()),
                }
            }
            _ => return None,
        };
        if let BedrockCredential::Profile(profile) = &credential
            && let Some(region) = self.profiles.iter().find_map(|discovered| {
                (discovered.name == *profile)
                    .then(|| discovered.region.clone())
                    .flatten()
            })
        {
            return Some(BedrockAction::Configure(credential, region));
        }
        self.enter_region(credential, String::new());
        None
    }
}

#[path = "bedrock_flow.rs"]
mod flow;
#[path = "bedrock_keys.rs"]
mod keys;
#[path = "bedrock_view.rs"]
mod view;
pub(super) use flow::BedrockFlow;
pub(super) use flow::FlowAction;

#[cfg(test)]
#[path = "bedrock_tests.rs"]
mod tests;
