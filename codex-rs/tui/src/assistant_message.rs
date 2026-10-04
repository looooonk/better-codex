use codex_app_server_protocol::AsyncUserInputQuestion;

pub(crate) fn with_questions(text: &str, questions: Option<&[AsyncUserInputQuestion]>) -> String {
    let mut text = text.to_string();
    for question in questions.into_iter().flatten() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&question.title);
        for option in question.options.iter().flatten() {
            text.push_str("\n- ");
            text.push_str(option);
        }
    }
    text
}
