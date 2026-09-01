use pretty_assertions::assert_eq;

use super::*;

fn params(config_paths: Vec<Vec<String>>) -> EnvironmentConfigReadParams {
    EnvironmentConfigReadParams {
        cwd: PathUri::from_host_native_path(std::env::current_dir().expect("current directory"))
            .expect("cwd URI"),
        config_paths,
        requirements_paths: Vec::new(),
    }
}

#[test]
fn selector_limits_accept_the_exact_bounds() {
    let selectors = (0..MAX_ENVIRONMENT_CONFIG_SELECTORS)
        .map(|index| vec![index.to_string()])
        .collect();
    validate_paths(&params(selectors)).expect("selector count at limit");
    validate_paths(&params(vec![
        vec!["x".to_string(); MAX_ENVIRONMENT_CONFIG_SELECTOR_COMPONENTS],
    ]))
    .expect("selector depth at limit");
    validate_paths(&params(vec![vec![
        "x".repeat(MAX_ENVIRONMENT_CONFIG_SELECTOR_BYTES),
    ]]))
    .expect("selector bytes at limit");
}

#[test]
fn selector_limits_reject_overflow() {
    let cases = [
        (
            params(Vec::new()),
            "at least one config or requirements path is required".to_string(),
        ),
        (
            params(vec![Vec::new()]),
            "TOML paths must contain at least one key segment".to_string(),
        ),
        (
            params(
                (0..=MAX_ENVIRONMENT_CONFIG_SELECTORS)
                    .map(|index| vec![index.to_string()])
                    .collect(),
            ),
            format!("at most {MAX_ENVIRONMENT_CONFIG_SELECTORS} TOML selectors are allowed"),
        ),
        (
            params(vec![vec![
                "x".to_string();
                MAX_ENVIRONMENT_CONFIG_SELECTOR_COMPONENTS + 1
            ]]),
            format!(
                "TOML paths may contain at most {MAX_ENVIRONMENT_CONFIG_SELECTOR_COMPONENTS} key segments"
            ),
        ),
        (
            params(vec![vec![
                "x".repeat(MAX_ENVIRONMENT_CONFIG_SELECTOR_BYTES + 1),
            ]]),
            format!(
                "TOML selector input may contain at most {MAX_ENVIRONMENT_CONFIG_SELECTOR_BYTES} bytes"
            ),
        ),
    ];

    for (params, expected) in cases {
        let error = validate_paths(&params).expect_err("invalid selectors should fail");
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn serialized_response_limit_is_exact() {
    let cwd = PathUri::from_host_native_path(std::env::current_dir().expect("current directory"))
        .expect("cwd URI");
    let mut response = EnvironmentConfigReadResponse {
        user_home_dir: None,
        codex_home_dir: cwd.clone(),
        hostname: None,
        config: EnvironmentConfigLayerStack {
            layers: vec![EnvironmentConfigLayer {
                source: "test".to_string(),
                base_dir: cwd,
                toml: String::new(),
            }],
            cloud_insertion_index: 0,
        },
        requirements: EnvironmentConfigLayerStack {
            layers: Vec::new(),
            cloud_insertion_index: 0,
        },
    };
    let fixed_bytes = serde_json::to_vec(&response)
        .expect("serialize response")
        .len();
    response.config.layers[0].toml =
        "x".repeat(MAX_ENVIRONMENT_CONFIG_RESPONSE_BYTES - fixed_bytes);
    assert_eq!(
        serde_json::to_vec(&response)
            .expect("serialize response at limit")
            .len(),
        MAX_ENVIRONMENT_CONFIG_RESPONSE_BYTES
    );
    validate_response_size(&response).expect("response at limit");

    response.config.layers[0].toml.push('x');
    let error = validate_response_size(&response).expect_err("oversized response should fail");
    assert_eq!(
        error.to_string(),
        format!(
            "environment config response exceeds the {MAX_ENVIRONMENT_CONFIG_RESPONSE_BYTES}-byte limit"
        )
    );
}
