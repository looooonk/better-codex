use std::collections::HashMap;

use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;

use super::*;

fn selection(environment_id: &str) -> TurnEnvironmentSelection {
    TurnEnvironmentSelection {
        environment_id: environment_id.to_string(),
        cwd: PathUri::parse("file:///workspace").expect("cwd URI"),
        workspace_roots: Vec::new(),
    }
}

#[test]
fn watcher_skips_remote_and_unknown_environments_before_local() {
    let environments = [
        selection("remote"),
        selection("missing"),
        selection("local"),
        selection("later-local"),
    ];
    let environment_types = HashMap::from([
        ("remote", true),
        ("local", false),
        ("later-local", false),
    ]);

    let selected = first_local_environment(
        &environments,
        |environment_id| {
            environment_types
                .get(environment_id)
                .copied()
                .map(|remote| (environment_id.to_string(), remote))
        },
        |(_, remote)| *remote,
    );

    assert_eq!(Some(("local".to_string(), false)), selected);
}
