use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::MultiAgentMode;
use codex_protocol::config_types::Settings;
use codex_utils_output_truncation::approx_token_count;

use super::*;
use crate::context::CollaborationModeInstructions;
use crate::context::ContextWindowGuidance;
use crate::context::MultiAgentModeInstructions;
use crate::context::RealtimeEndInstructions;
use crate::context::RealtimeStartWithInstructions;

#[test]
fn configured_developer_fragments_share_a_hard_cap() {
    let oversized = format!("prefix\n{}\nsuffix", "x".repeat(50_000));
    let collaboration_mode = CollaborationMode {
        mode: ModeKind::Default,
        settings: Settings {
            model: "test-model".to_string(),
            reasoning_effort: None,
            developer_instructions: Some(oversized.clone()),
        },
    };
    let rendered = [
        DeveloperInstructions::new(oversized.as_str()).render(),
        CollaborationModeInstructions::from_collaboration_mode(&collaboration_mode)
            .expect("collaboration instructions")
            .render(),
        MultiAgentModeInstructions::new(MultiAgentMode::Custom(oversized.clone())).render(),
        RealtimeStartWithInstructions::new(oversized.as_str()).render(),
        RealtimeEndInstructions::new(oversized.as_str()).render(),
        ContextWindowGuidance::new(oversized.as_str()).render(),
    ];

    for fragment in rendered {
        assert!(approx_token_count(&fragment) <= DEVELOPER_CONFIGURATION_MAX_TOKENS);
        assert!(fragment.contains("prefix"));
        assert!(fragment.contains("suffix"));
    }
}
