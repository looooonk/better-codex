use codex_app_server_protocol::AsyncUserInputQuestion;

pub(crate) fn with_questions(text: &str, questions: Option<&[AsyncUserInputQuestion]>) -> String {
    let question_text = questions
        .into_iter()
        .flatten()
        .map(|question| {
            let mut prompt = question.title.clone();
            for option in question.options.iter().flatten() {
                prompt.push_str("\n- ");
                prompt.push_str(option);
            }
            prompt
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if question_text.is_empty() || text.trim_end().ends_with(&question_text) {
        text.to_owned()
    } else if text.is_empty() {
        question_text
    } else {
        format!("{text}\n\n{question_text}")
    }
}
