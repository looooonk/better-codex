#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::app_shell) struct DiffMetadata {
    mode: Option<MetadataTransition>,
    binary_oid: Option<MetadataTransition>,
    unknown: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MetadataTransition {
    old: Option<String>,
    new: Option<String>,
}

impl DiffMetadata {
    pub(super) fn from_diff(diff: &str) -> Self {
        let lines = diff.lines().collect::<Vec<_>>();
        let mode = mode_transition(&lines);
        let is_binary = lines
            .iter()
            .any(|line| line.starts_with("Binary files ") || *line == "GIT binary patch");
        let binary_oid = if is_binary {
            binary_oid_transition(&lines)
        } else {
            None
        };
        let unknown = is_binary && binary_oid.is_none();
        Self {
            mode,
            binary_oid,
            unknown,
        }
    }

    pub(super) fn from_rowless_modified_diff(diff: &str) -> Self {
        let metadata = Self::from_diff(diff).for_existing_file();
        if metadata.mode.is_none() && metadata.binary_oid.is_none() {
            metadata.with_unknown()
        } else {
            metadata
        }
    }

    pub(super) fn for_existing_file(mut self) -> Self {
        self.unknown |= [&self.mode, &self.binary_oid]
            .into_iter()
            .flatten()
            .any(|transition| transition.old.is_none() || transition.new.is_none());
        self
    }

    pub(super) fn for_added_file(mut self) -> Self {
        for transition in [&mut self.mode, &mut self.binary_oid].into_iter().flatten() {
            transition.old = None;
        }
        self
    }

    pub(super) fn for_deleted_file(mut self) -> Self {
        for transition in [&mut self.mode, &mut self.binary_oid].into_iter().flatten() {
            transition.new = None;
        }
        self
    }

    pub(in crate::app_shell) fn composed(
        mode: Option<(Option<String>, Option<String>)>,
        binary_oid: Option<(Option<String>, Option<String>)>,
    ) -> Self {
        Self {
            mode: mode.map(|(old, new)| MetadataTransition { old, new }),
            binary_oid: binary_oid.map(|(old, new)| MetadataTransition { old, new }),
            unknown: false,
        }
    }

    pub(in crate::app_shell) fn with_unknown(mut self) -> Self {
        self.unknown = true;
        self
    }

    pub(in crate::app_shell) fn mode_transition(&self) -> Option<(Option<&str>, Option<&str>)> {
        self.mode
            .as_ref()
            .map(|transition| (transition.old.as_deref(), transition.new.as_deref()))
    }

    pub(in crate::app_shell) fn binary_oid_transition(
        &self,
    ) -> Option<(Option<&str>, Option<&str>)> {
        self.binary_oid
            .as_ref()
            .map(|transition| (transition.old.as_deref(), transition.new.as_deref()))
    }

    pub(in crate::app_shell) fn is_unknown(&self) -> bool {
        self.unknown
    }

    pub(super) fn retained_text_bytes(&self) -> usize {
        [&self.mode, &self.binary_oid]
            .into_iter()
            .flatten()
            .flat_map(|transition| [&transition.old, &transition.new])
            .flatten()
            .map(String::len)
            .sum()
    }
}

fn mode_transition(lines: &[&str]) -> Option<MetadataTransition> {
    let old = lines.iter().find_map(|line| {
        line.strip_prefix("old mode ")
            .or_else(|| line.strip_prefix("deleted file mode "))
    });
    let new = lines.iter().find_map(|line| {
        line.strip_prefix("new mode ")
            .or_else(|| line.strip_prefix("new file mode "))
    });
    if old.is_none() && new.is_none() {
        return None;
    }
    Some(MetadataTransition {
        old: valid_metadata_value(old, valid_mode)?,
        new: valid_metadata_value(new, valid_mode)?,
    })
}

fn valid_metadata_value(
    value: Option<&str>,
    valid: impl Fn(&str) -> bool,
) -> Option<Option<String>> {
    match value {
        Some(value) if valid(value) => Some(Some(value.to_string())),
        Some(_) => None,
        None => Some(None),
    }
}

fn valid_mode(mode: &str) -> bool {
    !mode.is_empty() && mode.len() <= 16 && mode.bytes().all(|byte| matches!(byte, b'0'..=b'7'))
}

fn binary_oid_transition(lines: &[&str]) -> Option<MetadataTransition> {
    let oids = lines
        .iter()
        .find_map(|line| line.strip_prefix("index "))?
        .split_whitespace()
        .next()?;
    let (old, new) = oids.split_once("..")?;
    Some(MetadataTransition {
        old: oid_endpoint(old)?,
        new: oid_endpoint(new)?,
    })
}

fn oid_endpoint(oid: &str) -> Option<Option<String>> {
    (!oid.is_empty() && oid.len() <= 64 && oid.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| (!oid.bytes().all(|byte| byte == b'0')).then(|| oid.to_ascii_lowercase()))
}
