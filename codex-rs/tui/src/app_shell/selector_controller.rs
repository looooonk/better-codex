use super::ShellState;
use super::backend::AppShellBackend;
use super::selector::ReasoningEffortValue;
use super::selector::SelectorOutcome;
use super::selector::SelectorState;
use super::selector::SelectorValue;
use super::selector::ServiceTierValue;
use codex_protocol::config_types::SERVICE_TIER_DEFAULT_REQUEST_VALUE;
use color_eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::layout::Position;
use ratatui::layout::Rect;

impl ShellState {
    pub(super) fn open_model_selector(&mut self) {
        let selector = SelectorState::models(&self.model, &self.available_models);
        self.open_selector(selector);
    }

    pub(super) fn open_reasoning_selector(&mut self) {
        let Some(preset) = self
            .available_models
            .iter()
            .find(|preset| preset.model == self.model)
        else {
            self.settings
                .set_error(format!("model metadata unavailable for `{}`", self.model));
            return;
        };
        let current = self.reasoning_effort.clone().map_or(
            ReasoningEffortValue::Default,
            ReasoningEffortValue::Explicit,
        );
        let selector =
            SelectorState::reasoning_efforts(&current, &preset.supported_reasoning_efforts);
        self.open_selector(selector);
    }

    pub(super) fn open_service_tier_selector(&mut self) {
        let Some(preset) = self
            .available_models
            .iter()
            .find(|preset| preset.model == self.model)
        else {
            self.settings
                .set_error(format!("model metadata unavailable for `{}`", self.model));
            return;
        };
        let current = match self.service_tier.as_deref() {
            Some(tier) if tier != SERVICE_TIER_DEFAULT_REQUEST_VALUE => {
                ServiceTierValue::Explicit(tier.to_string())
            }
            Some(_) | None => ServiceTierValue::Default,
        };
        let selector = SelectorState::service_tiers(&current, &preset.service_tiers);
        self.open_selector(selector);
    }

    pub(super) fn open_approval_selector(&mut self) {
        self.open_selector(SelectorState::approval_policies(self.approval_policy));
    }

    pub(super) async fn handle_selector_key<S>(
        &mut self,
        key: KeyEvent,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        let outcome = self
            .selector
            .as_mut()
            .map(|selector| selector.handle_key(key))
            .unwrap_or(SelectorOutcome::Pending);
        self.resolve_selector_outcome(outcome, app_server).await
    }

    pub(super) async fn handle_selector_click<S>(
        &mut self,
        area: Rect,
        position: Position,
        app_server: &mut S,
    ) -> Result<bool>
    where
        S: AppShellBackend,
    {
        let Some(selector) = &mut self.selector else {
            return Ok(false);
        };
        let outcome = selector.select_at(area, position);
        self.resolve_selector_outcome(outcome, app_server).await?;
        Ok(true)
    }

    fn open_selector(&mut self, selector: SelectorState<SelectorValue>) {
        self.close_agent_log();
        self.command_palette = None;
        self.selector = Some(selector);
        self.clear_transcript_selection();
    }

    async fn resolve_selector_outcome<S>(
        &mut self,
        outcome: SelectorOutcome<SelectorValue>,
        app_server: &mut S,
    ) -> Result<()>
    where
        S: AppShellBackend,
    {
        let value = match outcome {
            SelectorOutcome::Pending => return Ok(()),
            SelectorOutcome::Cancelled => {
                self.selector = None;
                return Ok(());
            }
            SelectorOutcome::Selected(value) => value,
        };
        self.selector = None;
        match value {
            SelectorValue::Model(model) => self.apply_model(model, app_server).await?,
            SelectorValue::ReasoningEffort(ReasoningEffortValue::Default) => {
                self.apply_reasoning_effort(None, app_server).await?;
            }
            SelectorValue::ReasoningEffort(ReasoningEffortValue::Explicit(effort)) => {
                self.apply_reasoning_effort(Some(effort), app_server)
                    .await?;
            }
            SelectorValue::ServiceTier(ServiceTierValue::Default) => {
                self.apply_service_tier(
                    Some(SERVICE_TIER_DEFAULT_REQUEST_VALUE.to_string()),
                    app_server,
                )
                .await?;
            }
            SelectorValue::ServiceTier(ServiceTierValue::Explicit(tier)) => {
                self.apply_service_tier(Some(tier), app_server).await?;
            }
            SelectorValue::ApprovalPolicy(policy) => {
                self.apply_approval_policy(policy, app_server).await?;
            }
        }
        Ok(())
    }
}
