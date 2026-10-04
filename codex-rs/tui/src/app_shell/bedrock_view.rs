use super::*;

impl BedrockState {
    pub(super) fn render(&self, area: Rect, buf: &mut Buffer, error: Option<String>) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let mut lines: Vec<Line> = vec![
            Line::from(vec!["> ".into(), "Set up Amazon Bedrock".bold()]),
            "".into(),
        ];
        let mut footer = Vec::new();
        match &self.view {
            BedrockView::CheckingGovCloud(_) => {
                lines = vec!["Checking for AWS GovCloud...".dim().into()];
            }
            BedrockView::GovCloudWarning(_) => {
                lines = vec!["Using Codex with AWS GovCloud".bold().into(), "".into()];
                lines.push(Line::from(vec![
                    "You are using Codex with AWS GovCloud. Please ensure you or your administrator have read the ".into(),
                    "application configuration and security guidance"
                        .cyan()
                        .underlined(),
                    " before proceeding.".into(),
                ]));
                footer.push("".into());
                footer.push("> Acknowledge".cyan().into());
            }
            BedrockView::Discovering(_) => {
                lines.push("  Checking for existing AWS credentials...".dim().into());
            }
            BedrockView::Configuring(_) => {
                lines.push("  Setting up Amazon Bedrock...".dim().into());
            }
            BedrockView::Methods(list) => {
                if *list == BedrockMethodList::Detected && self.profiles.len() == 1 {
                    let profile = &self.profiles[0];
                    lines.push(format!("  AWS profile detected: {}", profile.name).into());
                    if let Some(region) = &profile.region {
                        lines.push(format!("  Region: {region}").dim().into());
                    }
                } else if *list == BedrockMethodList::Detected && self.profiles.len() > 1 {
                    lines.push("  Choose an AWS profile.".into());
                } else if *list == BedrockMethodList::Detected
                    && !self.environment_credentials.is_empty()
                {
                    lines.push("  AWS credentials detected in your environment.".into());
                } else {
                    if *list == BedrockMethodList::Detected {
                        lines.push("  No AWS credentials found.".into());
                    }
                    lines.push("  Choose how you authenticate with AWS.".into());
                }
                lines.push("".into());
                self.render_methods(&mut lines);
            }
            BedrockView::ProfileEntry(value) => {
                lines.push("  Enter the name of your AWS profile.".into());
                lines.push("".into());
                lines.push(Line::from(vec![
                    "  AWS profile: ".into(),
                    value.clone().cyan(),
                ]));
            }
            BedrockView::ApiKeyEntry(value) => {
                lines.push("  Enter your Amazon Bedrock API key.".into());
                lines.push("".into());
                let mut masked_value = "•".repeat(value.chars().count().saturating_sub(1));
                if let Some(character) = value.chars().last() {
                    masked_value.push(character);
                }
                lines.push(Line::from(vec![
                    "  Bedrock API key: ".into(),
                    masked_value.cyan(),
                ]));
            }
            BedrockView::RegionEntry { value, .. } => {
                lines.push("  Enter the AWS Region to use with Amazon Bedrock.".into());
                lines.push("".into());
                lines.push(Line::from(vec![
                    "  AWS Region: ".into(),
                    value.clone().cyan(),
                ]));
            }
            BedrockView::AccessKeyEntry {
                values,
                selected_field,
            } => {
                lines.push("  Enter your AWS access keys.".into());
                lines.push("".into());
                for (index, label) in [
                    "AWS access key ID",
                    "AWS secret access key",
                    "AWS session token (optional)",
                ]
                .into_iter()
                .enumerate()
                {
                    let marker = if index == *selected_field { ">" } else { " " };
                    let value = if index == 0 {
                        values[index].clone()
                    } else if index == *selected_field {
                        let mut masked_value =
                            "•".repeat(values[index].chars().count().saturating_sub(1));
                        if let Some(character) = values[index].chars().last() {
                            masked_value.push(character);
                        }
                        masked_value
                    } else {
                        "•".repeat(values[index].chars().count())
                    };
                    let line = format!("{marker} {label}: {value}");
                    lines.push(if index == *selected_field {
                        line.cyan().into()
                    } else {
                        line.into()
                    });
                }
            }
            BedrockView::EnvironmentInstructions => {
                lines.push(
                    "  Configure AWS credentials in your environment, then restart Codex.".into(),
                );
                lines.push("".into());
                lines.push(Line::from(vec![
                    "  Setup guide: ".into(),
                    "https://learn.chatgpt.com/docs/amazon-bedrock"
                        .cyan()
                        .underlined(),
                ]));
                lines.push("".into());
                self.render_methods(&mut lines);
            }
        }
        if !matches!(
            self.view,
            BedrockView::Discovering(_) | BedrockView::CheckingGovCloud(_)
        ) {
            footer.push("".into());
            if !matches!(self.view, BedrockView::Configuring(_)) {
                footer.push(Line::from(vec![
                    "  Press ".dim(),
                    keys::CONFIRM[0].into(),
                    " to continue".dim(),
                ]));
            }
            if matches!(self.view, BedrockView::Configuring(_)) {
                footer.push("  Ctrl+C to exit".dim().into());
            } else if !matches!(self.view, BedrockView::GovCloudWarning(_)) {
                footer.push(Line::from(vec![
                    "  Press ".dim(),
                    keys::CANCEL[0].into(),
                    " to go back".dim(),
                ]));
            }
        }
        if let Some(error) = error {
            footer.push("".into());
            footer.push(error.red().into());
        }
        let mut lines = word_wrap_lines(lines, usize::from(area.width));
        let mut footer = word_wrap_lines(footer, usize::from(area.width));
        if matches!(self.view, BedrockView::GovCloudWarning(_))
            && lines.len() + footer.len() > usize::from(area.height)
        {
            footer = vec!["> Acknowledge".cyan().into()];
            if area.height > 2 {
                footer.push(Line::from(vec![
                    keys::MOVE_UP[0].into(),
                    "/".dim(),
                    keys::MOVE_DOWN[0].into(),
                    " scroll · ".dim(),
                    keys::CONFIRM[0].into(),
                ]));
            }
            footer = word_wrap_lines(footer, usize::from(area.width));
        }
        if lines.len() + footer.len() <= usize::from(area.height) || footer.is_empty() {
            lines.extend(footer);
            Paragraph::new(lines).render(area, buf);
            if let BedrockView::GovCloudWarning(scroll) = &self.view {
                scroll.store(0, Ordering::Relaxed);
                crate::terminal_hyperlinks::mark_underlined_hyperlink(
                    buf,
                    area,
                    GOV_CLOUD_GUIDANCE_URL,
                );
            }
            return;
        }
        let footer_height = u16::try_from(footer.len())
            .unwrap_or(u16::MAX)
            .min(area.height.saturating_sub(1));
        let content_height = area.height.saturating_sub(footer_height);
        let highlighted_row = lines
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(index, line)| {
                line.spans
                    .first()
                    .is_some_and(|span| span.content.starts_with("> "))
                    .then_some(index)
            })
            .unwrap_or_default();
        let max_scroll = lines.len().saturating_sub(usize::from(content_height));
        let scroll = if let BedrockView::GovCloudWarning(scroll) = &self.view {
            let offset = scroll.load(Ordering::Relaxed).min(max_scroll);
            scroll.store(offset, Ordering::Relaxed);
            offset
        } else {
            highlighted_row
                .saturating_add(2)
                .saturating_sub(usize::from(content_height))
                .min(max_scroll)
        };
        let content_area = Rect {
            height: content_height,
            ..area
        };
        let footer_area = Rect {
            y: area.y.saturating_add(content_height),
            height: footer_height,
            ..area
        };
        Paragraph::new(lines)
            .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
            .render(content_area, buf);
        Paragraph::new(footer).render(footer_area, buf);
        if matches!(self.view, BedrockView::GovCloudWarning(_)) {
            crate::terminal_hyperlinks::mark_underlined_hyperlink(
                buf,
                content_area,
                GOV_CLOUD_GUIDANCE_URL,
            );
        }
    }

    fn render_methods(&self, lines: &mut Vec<Line<'static>>) {
        for (index, method) in self.methods().into_iter().enumerate() {
            let (title, description) = match method {
                BedrockMethod::Profile(profile_index) => {
                    let profile = &self.profiles[profile_index];
                    let title = if self.profiles.len() == 1 {
                        format!("Continue with {}", profile.name)
                    } else {
                        profile.name.clone()
                    };
                    let description = if self.profiles.len() == 1 {
                        "Use your existing AWS credentials".to_string()
                    } else {
                        profile.region.clone().unwrap_or_default()
                    };
                    (title, description)
                }
                BedrockMethod::Environment => {
                    let description = if self.environment_credentials.iter().any(|credential| {
                        credential.credential_type == AwsCredentialType::BedrockApiKey
                    }) {
                        "Use your existing Amazon Bedrock API key"
                    } else {
                        "Use your existing AWS credentials"
                    };
                    (
                        "Continue with detected credentials".to_string(),
                        description.to_string(),
                    )
                }
                BedrockMethod::OtherMethods => (
                    if matches!(self.view, BedrockView::EnvironmentInstructions) {
                        "Choose another sign-in method"
                    } else {
                        "Other AWS sign-in methods"
                    }
                    .to_string(),
                    if matches!(self.view, BedrockView::EnvironmentInstructions) {
                        ""
                    } else {
                        "Use another profile, access keys, or environment variables"
                    }
                    .to_string(),
                ),
                BedrockMethod::ManualProfile => (
                    "AWS profile".to_string(),
                    "Use AWS SSO or a named profile".to_string(),
                ),
                BedrockMethod::AccessKeys => (
                    "AWS access keys".to_string(),
                    "Enter an access key ID and secret access key".to_string(),
                ),
                BedrockMethod::EnvironmentInstructions => (
                    "Environment variables".to_string(),
                    "Configure AWS credentials in your environment, then return here".to_string(),
                ),
                BedrockMethod::ApiKey => (
                    "Bedrock API key".to_string(),
                    "Enter a Bedrock API key".to_string(),
                ),
            };
            let selected = index == self.highlighted;
            let marker = if selected { ">" } else { " " };
            let title_line = format!("{marker} {}. {title}", index + 1);
            lines.push(if selected {
                title_line.cyan().into()
            } else {
                title_line.into()
            });
            if !description.is_empty()
                && (!matches!(self.view, BedrockView::Methods(BedrockMethodList::Detected))
                    || !self.profiles.is_empty()
                    || self.environment_credentials.is_empty())
            {
                lines.push(format!("     {description}").dim().into());
            }
            lines.push("".into());
        }
    }
}
