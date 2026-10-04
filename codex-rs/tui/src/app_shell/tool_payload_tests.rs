use super::*;
use crate::app_shell::CompletedItemOrigin;
use crate::app_shell::ShellState;
use crate::app_shell::TranscriptKind;
use crate::app_shell::transcript_view::render_transcript_line;
use codex_app_server_protocol::ThreadItem;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::path::Path;

fn completed_items() -> Vec<ThreadItem> {
    [
        json!({"type":"mcpToolCall", "id":"mcp", "server":"files", "tool":"read", "status":"completed", "arguments":{},
            "result":{"content":[{"type":"text","text":"Report ready."}, {"type":"resource_link","name":"Report","uri":"file:///remote/report.pdf"}], "structuredContent":{"count":3}, "_meta":null}}),
        json!({"type":"dynamicToolCall", "id":"dynamic", "tool":"create", "status":"completed", "arguments":{}, "success":true,
            "contentItems":[{"type":"inputText","text":"Created a preview."}, {"type":"inputImage","imageUrl":"data:image/png;base64,c2VjcmV0"}, {"type":"inputAudio","audioUrl":"https://example.com/audio.wav"}]}),
        json!({"type":"functionCallOutput", "id":"function", "name":"lookup", "namespace":"functions", "output":[
            {"type":"input_text","text":"Tool result."}, {"type":"input_image","file_id":"file-image-123"}]}),
        json!({"type":"imageGeneration", "id":"generated", "status":"completed", "result":"", "revisedPrompt":"A quiet garden.", "savedPath":"/workspace/garden.png", "transparentBackground":true}),
        json!({"type":"imageGeneration", "id":"failed", "status":"failed", "result":"", "failure":{"type":"usageLimitExceeded","limitId":"image-generation","resetsAt":1791046800}}),
        json!({"type":"webSearch", "id":"search", "query":"native features", "results":[{"title":"Release notes","url":"https://example.com/release","snippet":"New capabilities."}]}),
    ].into_iter().map(|item| serde_json::from_value(item).unwrap()).collect()
}

fn shell_with_items(origin: CompletedItemOrigin) -> ShellState {
    let mut shell = ShellState::snapshot_fixture();
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    for item in completed_items() {
        shell.ingest_completed_item(item, origin);
    }
    shell
}

#[test]
fn completed_and_restored_tools_preserve_payloads_in_expandable_cards() {
    let mut live = shell_with_items(CompletedItemOrigin::Live);
    let restored = shell_with_items(CompletedItemOrigin::Historical);
    assert_eq!(live.transcript, restored.transcript);
    let output_indices: Vec<_> = live
        .transcript
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (line.kind == TranscriptKind::Output).then_some(index))
        .collect();
    assert_eq!(output_indices.len(), completed_items().len());
    for index in output_indices {
        let expected = live.transcript[index]
            .full_text
            .as_ref()
            .unwrap()
            .to_string();
        assert!(live.open_tool_output_at(index));
        assert_eq!(live.tool_output.as_ref().unwrap().output(), expected);
    }
    let rendered = live
        .transcript
        .iter()
        .flat_map(|line| {
            render_transcript_line(
                line.kind,
                &line.text,
                line.tool_status,
                /*width*/ 90,
                Path::new("/workspace"),
                /*selected*/ false,
            )
        })
        .map(|line| {
            line.line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(rendered);
}

#[test]
fn media_content_is_identifiable_without_dumping_embedded_bytes() {
    let result = McpToolCallResult {
        content: vec![
            json!({"type":"image","mimeType":"image/png","data":"secret-image-bytes"}),
            json!({"type":"audio","mimeType":"audio/wav","data":"secret-audio-bytes"}),
            json!({"type":"resource","resource":{"uri":"resource://document","text":"Document content"}}),
            json!({"type":"resource","resource":{"uri":"resource://archive","mimeType":"application/zip","blob":"secret-archive-bytes"}}),
        ],
        structured_content: None,
        meta: None,
    };
    assert_eq!(
        mcp_output(&result).unwrap().to_string(),
        "Image: [inline image/png attachment]\nAudio: [inline audio/wav attachment]\nResource: resource://document\nDocument content\nResource: resource://archive\nAttachment: application/zip"
    );
    let output = FunctionCallOutputBody::ContentItems(vec![
        FunctionCallOutputContentItem::InputAudio {
            audio_url: "data:audio/wav;base64,secret-audio-bytes".into(),
        },
        FunctionCallOutputContentItem::EncryptedContent {
            encrypted_content: "encrypted-payload".into(),
        },
    ]);
    assert_eq!(
        function_output(&output).unwrap().to_string(),
        "Audio: [inline audio/wav attachment]\n[Encrypted tool content]"
    );
}

#[test]
fn large_payloads_use_existing_viewer_limits_and_bound_structured_serialization() {
    let output = function_output(&FunctionCallOutputBody::Text(
        "many lines of output\n".repeat(40_000),
    ))
    .unwrap();
    assert!(output.is_truncated());
    assert!(output.len() < MAX_JSON_BYTES);
    let output = web_results(&[json!({"large":"x".repeat(MAX_JSON_BYTES * 2)})]).unwrap();
    assert!(output.len() <= MAX_JSON_BYTES);
    assert!(output.ends_with("... structured result truncated ..."));
    let output = dynamic_output(&vec![
        DynamicToolCallOutputContentItem::InputText {
            text: "line".into()
        };
        MAX_CONTENT_BLOCKS + 2
    ])
    .unwrap();
    assert!(output.ends_with("... 2 additional content blocks omitted ..."));
}

#[test]
fn generated_remote_images_never_suggest_a_local_file_url() {
    let item = completed_items()
        .into_iter()
        .find_map(|item| match item {
            ThreadItem::ImageGeneration(item) if item.saved_path.is_some() => Some(item),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        image_generation(&item, ArtifactHost::Remote)
            .unwrap()
            .to_string(),
        "A quiet garden.\nSaved on remote host: /workspace/garden.png\nTransparent background"
    );
}
