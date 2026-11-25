use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

fn present(prompts: &mut SharedSettingsPrompts, method: &str, params: Value) -> Value {
    let request =
        serde_json::from_value(json!({"id": 7, "method": method, "params": params})).unwrap();
    let mut event = AppServerEvent::ServerRequest(Box::new(request));
    prompts.observe(&mut event);
    let AppServerEvent::ServerRequest(request) = event else {
        unreachable!()
    };
    serde_json::to_value(*request).unwrap()["params"].clone()
}

#[test]
fn remote_mcp_approval_keeps_session_scope_and_rejects_persistent_replies() {
    let mut prompts = SharedSettingsPrompts::default();
    let id = RequestId::Integer(7);
    let form = |kind, persist| {
        json!({
            "threadId": "thread", "serverName": "mcp", "mode": "form", "message": "Allow?",
            "requestedSchema": {"type":"object", "properties":{}},
            "_meta": {"codex_approval_kind": kind, "persist": persist}
        })
    };
    let method = "mcpServer/elicitation/request";
    let params = present(
        &mut prompts,
        method,
        form("mcp_tool_call", json!(["session", "always"])),
    );
    assert_eq!(params["_meta"]["persist"], json!(["session"]));
    for reply in [
        json!({"action":"accept","_meta":{"persist":"always"}}),
        json!({"action":"accept","content":{"mcp_tool_call_approval_123": [REMEMBER_ANSWER]}}),
    ] {
        assert_eq!(
            prompts.resolve(&id, &reply).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
    prompts
        .resolve(
            &id,
            &json!({"action":"accept","_meta":{"persist":"session"}}),
        )
        .unwrap();

    let params = present(
        &mut prompts,
        method,
        form("tool_suggestion", json!("always")),
    );
    assert!(params["_meta"].get("persist").is_none());
    assert!(
        prompts
            .resolve(
                &id,
                &json!({"action":"decline","_meta":{"persist":"always"}})
            )
            .is_err()
    );
    prompts.resolve(&id, &json!({"action":"decline"})).unwrap();

    let params = present(&mut prompts, method, form("custom_form", json!("always")));
    assert_eq!(params["_meta"]["persist"], "always");
    prompts
        .resolve(
            &id,
            &json!({"action":"accept","_meta":{"persist":"always"}}),
        )
        .unwrap();
}

#[test]
fn stock_command_approval_preserves_once_and_session_choices() {
    let mut prompts = SharedSettingsPrompts::default();
    let id = RequestId::Integer(7);
    let amendment = json!({"acceptWithExecpolicyAmendment": {"execpolicy_amendment": ["echo"]}});
    let params = present(
        &mut prompts,
        "item/commandExecution/requestApproval",
        json!({
            "threadId":"thread","turnId":"turn","itemId":"item","startedAtMs":1,
            "proposedExecpolicyAmendment": ["echo"],
            "availableDecisions": ["accept", "acceptForSession", amendment, "decline"],
        }),
    );
    assert!(params.get("proposedExecpolicyAmendment").is_none());
    assert_eq!(
        params["availableDecisions"],
        json!(["accept", "acceptForSession", "decline"])
    );
    assert!(
        prompts
            .resolve(&id, &json!({"decision":amendment}))
            .is_err()
    );
    prompts
        .resolve(&id, &json!({"decision":"acceptForSession"}))
        .unwrap();
}

#[test]
fn legacy_question_filters_only_the_codex_mcp_approval_answer() {
    let mut prompts = SharedSettingsPrompts::default();
    let id = RequestId::Integer(7);
    let question = "mcp_tool_call_approval_123";
    let questions = [question, "survey"].map(|id| json!({
        "id":id,"header":"Choice","question":"Allow?",
        "options":[{"label":"Allow once","description":""},{"label":REMEMBER_ANSWER,"description":""}]
    }));
    let params = present(
        &mut prompts,
        "item/tool/requestUserInput",
        json!({
            "threadId":"thread","turnId":"turn","itemId":"item",
            "questions": questions
        }),
    );
    assert_eq!(
        params["questions"][0]["options"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        params["questions"][1]["options"].as_array().unwrap().len(),
        2
    );
    assert!(
        prompts
            .resolve(
                &id,
                &json!({"answers":{question:{"answers":[REMEMBER_ANSWER]}}})
            )
            .is_err()
    );
    prompts
        .resolve(
            &id,
            &json!({"answers":{"survey":{"answers":[REMEMBER_ANSWER]}}}),
        )
        .unwrap();
}
