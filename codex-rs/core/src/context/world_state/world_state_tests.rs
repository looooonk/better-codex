use super::*;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;

#[derive(Clone, Deserialize, Serialize)]
struct TestSection {
    value: String,
    optional: Option<String>,
    array: Vec<Value>,
}

impl WorldStateSection for TestSection {
    const ID: &'static str = "test";
    type Snapshot = Self;

    fn snapshot(&self) -> Self::Snapshot {
        self.clone()
    }

    fn render_diff(
        &self,
        previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> Option<Box<dyn ContextualUserFragment>> {
        match previous {
            PreviousSectionState::Known(previous) if self.value != previous.value => {
                Some(Box::new(TestFragment(self.value.clone())))
            }
            PreviousSectionState::Unknown | PreviousSectionState::Stale => {
                Some(Box::new(TestFragment("unknown".to_string())))
            }
            PreviousSectionState::Absent | PreviousSectionState::Known(_) => None,
        }
    }
}

struct TestFragment(String);

impl ContextualUserFragment for TestFragment {
    fn role(&self) -> &'static str {
        "user"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("", "")
    }

    fn body(&self) -> String {
        self.0.clone()
    }
}

#[test]
fn world_state_hash_normalizes_crlf_line_endings() {
    assert_eq!(
        WorldStateHash::from_fragment(&TestFragment("line one\r\nline two".to_string())),
        WorldStateHash::from_fragment(&TestFragment("line one\nline two".to_string())),
    );
}

struct ExtensionTestFragment(&'static str);

impl ContextualUserFragment for ExtensionTestFragment {
    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<extension_test>", "</extension_test>")
    }

    fn body(&self) -> String {
        self.0.to_string()
    }
}

struct DuplicateTestSection;

impl WorldStateSection for DuplicateTestSection {
    const ID: &'static str = "test";
    type Snapshot = ();

    fn snapshot(&self) -> Self::Snapshot {}

    fn render_diff(
        &self,
        _previous: PreviousSectionState<'_, Self::Snapshot>,
    ) -> Option<Box<dyn ContextualUserFragment>> {
        None
    }
}

#[test]
fn snapshot_uses_stable_section_ids_and_omits_null_fields() {
    let mut world_state = WorldState::default();
    world_state.add_section(TestSection {
        value: "current".to_string(),
        optional: None,
        array: vec![json!({"value": null})],
    });

    assert_eq!(
        serde_json::to_value(world_state.snapshot()).expect("serialize world-state snapshot"),
        json!({"test": {"value": "current", "array": [{"value": null}]}})
    );
}

#[test]
fn render_diff_restores_the_typed_section_snapshot() {
    let mut previous = WorldState::default();
    previous.add_section(TestSection {
        value: "before".to_string(),
        optional: None,
        array: Vec::new(),
    });
    let mut current = WorldState::default();
    current.add_section(TestSection {
        value: "after".to_string(),
        optional: None,
        array: Vec::new(),
    });

    let rendered = current.render_diff(&previous.snapshot());

    assert_eq!(
        vec!["after"],
        rendered
            .into_iter()
            .map(|fragment| fragment.body())
            .collect::<Vec<_>>()
    );
}

#[test]
fn extension_owned_section_uses_its_snapshot_and_renderer() {
    let mut world_state = WorldState::default();
    world_state.add_extension_section(WorldStateSectionContribution::new(
        "extension_test",
        json!({"value": "after", "optional": null}),
        |previous| match previous {
            PreviousWorldStateSection::Known(previous)
                if previous == &json!({"value": "before"}) =>
            {
                Some(RenderedWorldStateFragment::new(ExtensionTestFragment(
                    "after",
                )))
            }
            PreviousWorldStateSection::Absent
            | PreviousWorldStateSection::Unknown
            | PreviousWorldStateSection::Known(_) => None,
        },
    ));
    let previous = WorldStateSnapshot {
        sections: BTreeMap::from([("extension_test".to_string(), json!({"value": "before"}))]),
    };

    let rendered = world_state.render_diff(&previous);

    assert_eq!(
        serde_json::to_value(world_state.snapshot()).expect("serialize world-state snapshot"),
        json!({"extension_test": {"value": "after"}})
    );
    assert_eq!(rendered.len(), 1);
    assert_eq!(rendered[0].role(), "developer");
    assert_eq!(
        rendered[0].render(),
        "<extension_test>after</extension_test>"
    );
}

#[test]
fn extension_world_state_keeps_every_rendered_fragment() {
    let section_ids = (0..40)
        .map(|index| {
            let id: &'static str = Box::leak(format!("extension_test_{index}").into_boxed_str());
            id
        })
        .collect::<Vec<_>>();
    let mut world_state = WorldState::default();
    for id in &section_ids {
        let body = *id;
        world_state.add_extension_section(WorldStateSectionContribution::new(
            id,
            json!({"body": body}),
            move |_| Some(RenderedWorldStateFragment::new(ExtensionTestFragment(body))),
        ));
    }

    assert_eq!(
        world_state
            .render_full()
            .into_iter()
            .map(|fragment| fragment.body())
            .collect::<Vec<_>>(),
        section_ids
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    );
}

#[test]
fn missing_retained_fragment_is_rendered_again() {
    let mut world_state = WorldState::default();
    world_state.add_extension_section(
        WorldStateSectionContribution::new(
            "extension_test",
            json!({"body": "current catalog"}),
            |previous| match previous {
                PreviousWorldStateSection::Absent | PreviousWorldStateSection::Unknown => Some(
                    RenderedWorldStateFragment::new(ExtensionTestFragment("current catalog")),
                ),
                PreviousWorldStateSection::Known(_) => None,
            },
        )
        .with_retained_fragment_matcher(|role, text| {
            role == "developer" && text.contains("current catalog")
        }),
    );
    let previous = WorldStateSnapshot {
        sections: BTreeMap::from([(
            "extension_test".to_string(),
            json!({"body": "stale catalog"}),
        )]),
    };
    let retained = ResponseItem::Message {
        id: None,
        role: "developer".to_string(),
        content: vec![ContentItem::InputText {
            text: "<extension_test>current catalog</extension_test>".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };

    assert_eq!(
        world_state
            .render_history_diff(Some(&previous), &[])
            .into_iter()
            .map(|fragment| fragment.body())
            .collect::<Vec<_>>(),
        vec!["current catalog"]
    );
    assert!(
        world_state
            .render_history_diff(Some(&previous), &[retained])
            .is_empty()
    );
}

#[test]
fn latest_matching_section_fragment_controls_retention_with_legacy_matcher() {
    let mut world_state = WorldState::default();
    world_state.add_extension_section(
        WorldStateSectionContribution::new(
            "extension_test",
            json!({"body": "current catalog"}),
            |previous| match previous {
                PreviousWorldStateSection::Known(previous)
                    if previous != &json!({"body": "current catalog"}) =>
                {
                    Some(RenderedWorldStateFragment::new(ExtensionTestFragment(
                        "current catalog",
                    )))
                }
                PreviousWorldStateSection::Absent | PreviousWorldStateSection::Unknown => Some(
                    RenderedWorldStateFragment::new(ExtensionTestFragment("current catalog")),
                ),
                PreviousWorldStateSection::Known(_) => None,
            },
        )
        .with_legacy_matcher(|role, text| {
            role == "developer"
                && text.starts_with("<extension_test>")
                && text.ends_with("</extension_test>")
        })
        .with_section_fragment_matcher(|role, text| {
            role == "developer"
                && text.starts_with("<extension_test>")
                && text.ends_with("</extension_test>")
        })
        .with_retained_fragment_matcher(|role, text| {
            role == "developer" && text == "<extension_test>current catalog</extension_test>"
        }),
    );
    let previous = WorldStateSnapshot {
        sections: BTreeMap::from([(
            "extension_test".to_string(),
            json!({"body": "current catalog"}),
        )]),
    };
    let message = |text: &str| ResponseItem::Message {
        id: None,
        role: "developer".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    let history = [
        message("<extension_test>current catalog</extension_test>"),
        message("<extension_test>current catalog plus extra</extension_test>"),
        message("<extension_test>current catalog</extension_test>"),
    ];

    assert!(
        world_state
            .render_history_diff(Some(&previous), &history)
            .is_empty()
    );

    assert_eq!(
        world_state
            .render_history_diff(Some(&previous), &history[..2])
            .into_iter()
            .map(|fragment| fragment.body())
            .collect::<Vec<_>>(),
        vec!["current catalog"]
    );
}

#[test]
fn stale_extension_fragment_reasserts_an_empty_current_value() {
    let mut world_state = WorldState::default();
    world_state.add_extension_section(
        WorldStateSectionContribution::new("extension_test", json!({}), |previous| {
            matches!(previous, PreviousWorldStateSection::Unknown)
                .then(|| RenderedWorldStateFragment::new(ExtensionTestFragment("")))
        })
        .with_section_fragment_matcher(|role, text| {
            role == "developer"
                && text.starts_with("<extension_test>")
                && text.ends_with("</extension_test>")
        })
        .with_retained_fragment_matcher(|role, text| {
            role == "developer" && text == "<extension_test></extension_test>"
        }),
    );
    let previous = WorldStateSnapshot {
        sections: BTreeMap::from([("extension_test".to_string(), json!({}))]),
    };
    let stale_fragment = ResponseItem::Message {
        id: None,
        role: "developer".to_string(),
        content: vec![ContentItem::InputText {
            text: "<extension_test>old value</extension_test>".to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };

    assert_eq!(
        world_state
            .render_history_diff(Some(&previous), &[stale_fragment])
            .into_iter()
            .map(|fragment| fragment.body())
            .collect::<Vec<_>>(),
        vec![String::new()]
    );
}

#[test]
fn unreadable_section_snapshot_is_treated_as_unknown() {
    let mut current = WorldState::default();
    current.add_section(TestSection {
        value: "current".to_string(),
        optional: None,
        array: Vec::new(),
    });
    let previous = WorldStateSnapshot {
        sections: BTreeMap::from([("test".to_string(), json!({"invalid": true}))]),
    };

    let rendered = current.render_diff(&previous);

    assert_eq!(
        vec!["unknown"],
        rendered
            .into_iter()
            .map(|fragment| fragment.body())
            .collect::<Vec<_>>()
    );
}

#[test]
#[should_panic(expected = "duplicate world-state section ID: test")]
fn duplicate_section_ids_are_rejected() {
    let mut world_state = WorldState::default();
    world_state.add_section(TestSection {
        value: "current".to_string(),
        optional: None,
        array: Vec::new(),
    });

    world_state.add_section(DuplicateTestSection);
}

#[test]
fn snapshot_merge_patch_changes_and_removes_nested_values() {
    let mut previous = WorldStateSnapshot {
        sections: BTreeMap::from([
            (
                "kept".to_string(),
                json!({"same": true, "changed": "before", "removed": true}),
            ),
            ("removed_section".to_string(), json!({"value": true})),
        ]),
    };
    let current = WorldStateSnapshot {
        sections: BTreeMap::from([(
            "kept".to_string(),
            json!({"same": true, "changed": "after"}),
        )]),
    };

    assert_eq!(
        current.merge_patch_from(&previous).map(Value::Object),
        Some(json!({
            "kept": {"changed": "after", "removed": null},
            "removed_section": null,
        }))
    );
    let patch = current
        .merge_patch_from(&previous)
        .expect("changed snapshots should produce a patch");
    previous.apply_merge_patch(&patch);
    assert_eq!(previous, current);
    assert_eq!(current.merge_patch_from(&current), None);
}
