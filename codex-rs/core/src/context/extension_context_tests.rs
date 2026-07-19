use codex_utils_string::approx_token_count;
use pretty_assertions::assert_eq;

use super::*;

struct TestExtensionFragment {
    role: &'static str,
    body: String,
}

impl ContextualUserFragment for TestExtensionFragment {
    fn role(&self) -> &'static str {
        self.role
    }

    fn markers(&self) -> (&'static str, &'static str) {
        ("<extension>", "</extension>")
    }

    fn body(&self) -> String {
        self.body.clone()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("<extension>", "</extension>")
    }
}

#[test]
fn admission_bounds_extension_context_by_tokens_and_count() {
    let mut budget = ExtensionContextBudget::default();
    let first = budget
        .admit(
            Box::new(TestExtensionFragment {
                role: "developer",
                body: "x".repeat(MAX_EXTENSION_CONTEXT_TOKENS * 8),
            }),
            Some("developer"),
        )
        .expect("first fragment should be admitted");
    assert!(approx_token_count(&first.render()) <= MAX_EXTENSION_CONTEXT_TOKENS);

    let second = budget.admit(
        Box::new(TestExtensionFragment {
            role: "developer",
            body: "second".to_string(),
        }),
        Some("developer"),
    );
    let admitted_tokens = approx_token_count(&first.render())
        + second
            .as_ref()
            .map(ContextualUserFragment::render)
            .as_deref()
            .map(approx_token_count)
            .unwrap_or_default();
    assert!(admitted_tokens <= MAX_EXTENSION_CONTEXT_TOKENS);

    let mut budget = ExtensionContextBudget::default();
    let admitted = (0..MAX_EXTENSION_CONTEXT_FRAGMENTS + 1)
        .filter_map(|_| {
            budget.admit(
                Box::new(TestExtensionFragment {
                    role: "user",
                    body: String::new(),
                }),
                Some("user"),
            )
        })
        .count();
    assert_eq!(admitted, MAX_EXTENSION_CONTEXT_FRAGMENTS);
}

#[test]
fn admission_rejects_invalid_or_mismatched_roles() {
    let mut budget = ExtensionContextBudget::default();
    for (role, expected_role) in [("system", None), ("user", Some("developer"))] {
        assert!(
            budget
                .admit(
                    Box::new(TestExtensionFragment {
                        role,
                        body: "body".to_string(),
                    }),
                    expected_role,
                )
                .is_none()
        );
    }
}
