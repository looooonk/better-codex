use ratatui::prelude::Stylize as _;
use ratatui::text::Line;
use ratatui::text::Span;

#[derive(Clone)]
pub(crate) struct ModelMigrationCopy {
    pub heading: Vec<Span<'static>>,
    pub content: Vec<Line<'static>>,
    pub can_opt_out: bool,
    pub markdown: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn migration_copy_for_models(
    current_model: &str,
    target_model: &str,
    model_link: Option<String>,
    migration_copy: Option<String>,
    migration_markdown: Option<String>,
    target_display_name: String,
    target_description: Option<String>,
    can_opt_out: bool,
) -> ModelMigrationCopy {
    if let Some(migration_markdown) = migration_markdown {
        return ModelMigrationCopy {
            heading: Vec::new(),
            content: Vec::new(),
            can_opt_out,
            markdown: Some(
                migration_markdown
                    .replace("{model_from}", current_model)
                    .replace("{model_to}", target_model),
            ),
        };
    }

    let heading = vec![
        format!("Better Codex just got an upgrade. Introducing {target_display_name}.").bold(),
    ];
    let description = migration_copy.clone().map(Line::from).unwrap_or_else(|| {
        target_description
            .filter(|description| !description.is_empty())
            .map(Line::from)
            .unwrap_or_else(|| {
                format!(
                    "{target_display_name} is recommended for better performance and reliability."
                )
                .into()
            })
    });

    let mut content = Vec::new();
    if migration_copy.is_none() {
        content.extend([
            format!("We recommend switching from {current_model} to {target_model}.").into(),
            Line::default(),
        ]);
    }
    if let Some(model_link) = model_link {
        content.extend([
            vec![
                format!("{description} Learn more about {target_display_name} at ").into(),
                model_link.cyan().underlined(),
            ]
            .into(),
            Line::default(),
        ]);
    } else {
        content.extend([description, Line::default()]);
    }
    content.push(if can_opt_out {
        format!("You can continue using {current_model} if you prefer.").into()
    } else {
        "Press enter to continue".dim().into()
    });

    ModelMigrationCopy {
        heading,
        content,
        can_opt_out,
        markdown: None,
    }
}
