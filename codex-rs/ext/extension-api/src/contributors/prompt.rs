use codex_context_fragments::ContextualUserFragment;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PromptSlot {
    DeveloperPolicy,
    DeveloperCapabilities,
    ContextualUser,
    SeparateDeveloper,
}

pub struct PromptFragment {
    slot: PromptSlot,
    fragment: Box<dyn ContextualUserFragment + Send>,
}

impl PromptFragment {
    /// Creates a prompt fragment for the given slot.
    ///
    /// The slot determines which top-level message receives the rendered fragment.
    pub fn new(slot: PromptSlot, fragment: impl ContextualUserFragment + Send + 'static) -> Self {
        Self {
            slot,
            fragment: Box::new(fragment),
        }
    }

    /// Creates a developer-policy prompt fragment.
    pub fn developer_policy(fragment: impl ContextualUserFragment + Send + 'static) -> Self {
        Self::new(PromptSlot::DeveloperPolicy, fragment)
    }

    /// Creates a developer-capabilities prompt fragment.
    pub fn developer_capability(fragment: impl ContextualUserFragment + Send + 'static) -> Self {
        Self::new(PromptSlot::DeveloperCapabilities, fragment)
    }

    /// Creates a separate top-level developer prompt fragment.
    pub fn separate_developer(fragment: impl ContextualUserFragment + Send + 'static) -> Self {
        Self::new(PromptSlot::SeparateDeveloper, fragment)
    }

    /// Returns the target prompt slot.
    pub fn slot(&self) -> PromptSlot {
        self.slot
    }

    /// Returns the rendered model-visible fragment.
    pub fn render(&self) -> String {
        self.fragment.render()
    }

    pub fn into_context_fragment(self) -> Box<dyn ContextualUserFragment + Send> {
        self.fragment
    }
}
