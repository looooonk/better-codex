use super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn context_with_fragment(fragment: &str) -> (TempDir, InlineVisualizationContext) {
    let codex_home = tempfile::tempdir().expect("temp codex home");
    let thread_id = ThreadId::new();
    let context = InlineVisualizationContext::new(codex_home.path(), thread_id)
        .expect("UUIDv7 thread id should provide a timestamp");
    fs::create_dir_all(&context.thread_dir).expect("create visualization directory");
    fs::write(context.thread_dir.join("chart.html"), fragment).expect("write fragment");
    (codex_home, context)
}

#[test]
fn granted_visualization_root_overrides_thread_id_derived_root() {
    let codex_home = tempfile::tempdir().expect("temp codex home");
    let granted_context = InlineVisualizationContext::new(codex_home.path(), ThreadId::new())
        .expect("granted context");
    fs::create_dir_all(&granted_context.thread_dir).expect("create granted directory");
    fs::write(
        granted_context.thread_dir.join("chart.html"),
        "<div>chart</div>",
    )
    .expect("write fragment");

    let context = InlineVisualizationContext::new_with_writable_roots(
        codex_home.path(),
        ThreadId::new(),
        [granted_context.thread_dir.as_path()],
    )
    .expect("context");

    assert!(context.link_for("chart.html").is_some());
}

fn line_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

#[test]
fn rewrites_complete_directive_to_trusted_static_file_placeholder() {
    let (_codex_home, context) = context_with_fragment("<div>chart</div>");

    let rewritten = rewrite_inline_visualizations(
        "Before\n::codex-inline-vis{file=\"chart.html\"}\nAfter",
        Some(&context),
    );

    assert!(
        rewritten
            .markdown
            .starts_with("Before\nOpen chart visualization in the browser  \n[")
    );
    assert!(rewritten.markdown.ends_with(")\nAfter"));
    assert_eq!(rewritten.trusted_file_links.len(), 1);
    let destination = rewritten
        .trusted_file_links
        .values()
        .next()
        .expect("trusted destination");
    assert_eq!(destination.destination.scheme(), "file");
    assert!(
        destination
            .destination
            .to_file_path()
            .expect("file URL")
            .is_file()
    );
    assert_eq!(
        destination.display_label,
        "Open chart visualization in the browser"
    );
    assert!(rewritten.markdown.contains(&format!(
        "  \n[{}](",
        destination.markdown_destination_label
    )));
}

#[test]
fn hides_incomplete_streaming_directive() {
    let rewritten = rewrite_inline_visualizations(
        "Before\n::codex-inline-vis{file=\"chart",
        /*context*/ None,
    );

    assert_eq!(rewritten.markdown, "Before\n");
    assert!(rewritten.trusted_file_links.is_empty());
}

#[test]
fn hides_incomplete_streaming_content_reference() {
    for reference in [
        "Before\n\u{e200}visualize\u{e202}{\"path\":\"/tmp/chart",
        "Before\n\u{e200}visualize\u{e202}{\"path\":\"/tmp/chart.html\"}",
    ] {
        let rewritten = rewrite_inline_visualizations(reference, /*context*/ None);

        assert_eq!(rewritten.markdown, "Before\n");
        assert!(rewritten.trusted_file_links.is_empty());
    }
}

#[test]
fn unavailable_or_invalid_content_reference_has_explicit_fallback() {
    let (_codex_home, context) = context_with_fragment("<div>chart</div>");
    let outside = tempfile::tempdir().expect("outside visualization directory");
    let outside_path = outside.path().join("chart.html");
    fs::write(&outside_path, "<div>outside</div>").expect("write outside fragment");

    let references = [
        serde_json::json!({ "path": outside_path }).to_string(),
        serde_json::json!({ "path": "chart.html" }).to_string(),
        "{\"path\":".to_string(),
    ];

    for payload in references {
        let reference = format!("\u{e200}visualize\u{e202}{payload}\u{e201}");
        assert_eq!(
            rewrite_inline_visualizations(&reference, Some(&context)).markdown,
            "_Visualization unavailable on this device._"
        );
    }
}

#[test]
fn unavailable_artifact_has_explicit_fallback() {
    let codex_home = tempfile::tempdir().expect("temp codex home");
    let context = InlineVisualizationContext::new(codex_home.path(), ThreadId::new())
        .expect("UUIDv7 thread id should provide a timestamp");

    assert_eq!(
        rewrite_inline_visualizations("::codex-inline-vis{file=\"missing.html\"}", Some(&context),)
            .markdown,
        "_Visualization unavailable on this device._"
    );
}

#[test]
fn rejects_parent_path_and_non_html_file() {
    let (_codex_home, context) = context_with_fragment("<div>chart</div>");

    for file in ["../chart.html", "chart.svg"] {
        assert_eq!(
            rewrite_inline_visualizations(
                &format!("::codex-inline-vis{{file=\"{file}\"}}"),
                Some(&context),
            )
            .markdown,
            "_Visualization unavailable on this device._"
        );
    }
}

#[test]
fn rejects_oversized_fragment() {
    let (_codex_home, context) = context_with_fragment("<div>chart</div>");
    let fragment = fs::OpenOptions::new()
        .write(true)
        .open(context.thread_dir.join("chart.html"))
        .expect("open fragment");
    fragment
        .set_len(MAX_FRAGMENT_BYTES + 1)
        .expect("enlarge fragment");

    assert_eq!(
        rewrite_inline_visualizations("::codex-inline-vis{file=\"chart.html\"}", Some(&context),)
            .markdown,
        "_Visualization unavailable on this device._"
    );
}

#[test]
fn viewer_materializes_sandboxed_static_document() {
    let (_codex_home, context) = context_with_fragment(
        "<div id=\"widget\"><div class=\"viz-controls\">controls</div><canvas id=\"chart\"></canvas></div><script>globalThis.chartRendered = true;</script>",
    );
    let url = context.link_for("chart.html").expect("visualization link");
    assert_eq!(url.scheme(), "file");
    let viewer_path = url.to_file_path().expect("viewer file path");
    assert_eq!(
        viewer_path.parent().and_then(Path::file_name),
        Some(std::ffi::OsStr::new(".codex-viewers"))
    );
    let document = fs::read_to_string(viewer_path).expect("read static viewer");

    assert!(document.contains("sandbox=\"allow-scripts\""));
    assert!(!document.contains("allow-same-origin"));
    assert!(document.contains("script-src 'unsafe-inline' 'unsafe-eval'"));
    assert!(document.contains(".viz-controls"));
    assert!(document.contains("https://unpkg.com/@floating-ui/dom@1.7.4"));
    assert!(document.contains("https://unpkg.com/lucide@1.17.0"));
    assert!(document.contains("&lt;canvas id=&quot;chart&quot;&gt;&lt;/canvas&gt;"));
    assert!(document.contains("globalThis.chartRendered = true"));
    assert!(document.contains("Content-Security-Policy"));

    let shell = document
        .split_once(" srcdoc=")
        .map(|(shell, _)| shell)
        .expect("viewer shell");
    let contract = format!(
        "{shell} srcdoc=\"[canonical visualization frame]\"></iframe></body></html>\n\nembedded frame:\n- canonical control styles\n- Floating UI tooltip runtime\n- Lucide icon runtime\n- visualization fragment"
    );
    insta::assert_snapshot!("viewer_document_contract", contract);
}

#[test]
fn viewer_reuses_path_and_refreshes_static_document() {
    let (_codex_home, context) = context_with_fragment("<div>first</div>");
    let first_url = context.link_for("chart.html").expect("first viewer link");
    let viewer_path = first_url.to_file_path().expect("viewer file path");
    let original_viewer_metadata = fs::metadata(&viewer_path).expect("read viewer metadata");
    assert!(
        fs::read_to_string(&viewer_path)
            .expect("read first viewer")
            .contains("first")
    );

    let reused_url = context.link_for("chart.html").expect("reused viewer link");

    assert_eq!(reused_url, first_url);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            fs::metadata(&viewer_path)
                .expect("read reused viewer metadata")
                .ino(),
            original_viewer_metadata.ino()
        );
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        assert_eq!(
            fs::metadata(&viewer_path)
                .expect("read reused viewer metadata")
                .creation_time(),
            original_viewer_metadata.creation_time()
        );
    }

    fs::write(context.thread_dir.join("chart.html"), "<div>second</div>").expect("update fragment");
    let second_url = context.link_for("chart.html").expect("second viewer link");

    assert_eq!(second_url, first_url);
    let refreshed = fs::read_to_string(viewer_path).expect("read refreshed viewer");
    assert!(refreshed.contains("second"));
    assert!(!refreshed.contains("first"));
}

#[test]
fn visualization_link_uses_the_artifact_name() {
    let (_codex_home, context) = context_with_fragment("<div>chart</div>");
    fs::rename(
        context.thread_dir.join("chart.html"),
        context.thread_dir.join("compound-interest-explorer.html"),
    )
    .expect("rename fragment");

    let rewritten = rewrite_inline_visualizations(
        "::codex-inline-vis{file=\"compound-interest-explorer.html\"}",
        Some(&context),
    );

    assert!(
        rewritten
            .markdown
            .starts_with("Open compound\\-interest\\-explorer visualization in the browser  \n[")
    );
    assert_eq!(
        rewritten
            .trusted_file_links
            .values()
            .next()
            .expect("trusted destination")
            .display_label,
        "Open compound-interest-explorer visualization in the browser"
    );
}

#[test]
fn user_markdown_keeps_directive_literal() {
    let rendered =
        crate::markdown_render::render_markdown_text("::codex-inline-vis{file=\"chart.html\"}");
    let text = rendered
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();

    assert_eq!(text, "::codex-inline-vis{file=\"chart.html\"}");
}

#[test]
fn themed_agent_renderer_keeps_trusted_visualizations_and_code_literals_distinct() {
    let (_home, context) = context_with_fragment("<div>chart</div>");
    let render = |source| {
        crate::markdown::render_markdown_agent_with_styles(
            source,
            Some(80),
            /*cwd*/ None,
            Some(&context),
            crate::markdown_render::ListSpacing::Uniform,
            crate::markdown_render::MarkdownStyles::default(),
        )
    };
    let rendered = render("::codex-inline-vis{file=\"chart.html\"}");
    let destination = rendered
        .iter()
        .flat_map(|line| &line.hyperlinks)
        .next()
        .unwrap();
    assert_eq!(
        Url::parse(&destination.terminal_destination().unwrap())
            .unwrap()
            .scheme(),
        "file"
    );
    let code = render("```text\n::codex-inline-vis{file=\"chart.html\"}\n```");
    assert!(code.iter().all(|line| line.hyperlinks.is_empty()));
    assert_eq!(
        line_text(&code[0].line),
        "::codex-inline-vis{file=\"chart.html\"}"
    );
    let visible = rendered
        .iter()
        .map(|line| line_text(&line.line))
        .collect::<Vec<_>>()
        .join("\n");
    let viewer_prefix =
        Url::from_directory_path(context.viewer_dir.join(".codex-viewers")).unwrap();
    insta::assert_snapshot!(visible.replace(viewer_prefix.as_str(), "file:///trusted-viewer/"));
}
