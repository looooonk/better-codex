use super::*;
use pretty_assertions::assert_eq;

#[test]
fn device_code_prompt_matches_expected_copy() {
    let prompt = device_code_prompt("https://example.com/device", "ABCD-EFGH");
    let version = env!("CARGO_PKG_VERSION");

    assert_eq!(
        prompt,
        format!(
            "\nWelcome to Better Codex [v\x1b[90m{version}\x1b[0m]\n\x1b[90mA terminal coding agent built on OpenAI's Codex backend\x1b[0m\n\
\nFollow these steps to sign in with ChatGPT using device code authorization:\n\
\n1. Open this link in your browser and sign in to your account\n   \
\x1b[94mhttps://example.com/device\x1b[0m\n\
\n2. Enter this one-time code \x1b[90m(expires in 15 minutes)\x1b[0m\n   \
\x1b[94mABCD-EFGH\x1b[0m\n\
\n\x1b[90mContinue only if you started this login in Better Codex. If a website or another person gave you this code, cancel.\x1b[0m\n"
        )
    );
}
