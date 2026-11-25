use super::*;
use crate::terminal_hyperlinks::HyperlinkLine;
use ratatui::style::Color;

const CELL: CellSize = CellSize {
    width: 9,
    height: 18,
};

#[test]
fn common_tex_rasterizes_in_text_and_display_styles() {
    for body in include_str!("corpus.txt").lines() {
        for display in [false, true] {
            let source = if display {
                format!("\\[{body}\\]")
            } else {
                format!("\\({body}\\)")
            };
            let m =
                raster::metrics(&source, display).unwrap_or_else(|| panic!("typesetting {source}"));
            let f = Formula::measure(
                m,
                source.clone().into(),
                display,
                CELL,
                160,
                raster::MathColors {
                    foreground: [200, 230, 255],
                    background: [0; 3],
                },
            )
            .unwrap();
            let pixels = f
                .rasterize()
                .unwrap_or_else(|| panic!("rasterizing {source}"));
            assert!(
                pixels.pixels().any(|p| p[3] > 100),
                "{source} has no visible ink"
            );
            assert_eq!(pixels.width(), u32::from(f.columns) * 9);
            assert_eq!(pixels.height(), u32::from(f.rows()) * 18);
        }
    }
}

#[test]
fn closed_delimiters_and_protected_markdown() {
    let mut metrics = Measurements::default();
    for (open, close) in [("$", "$"), ("\\(", "\\)"), ("$$", "$$"), ("\\[", "\\]")] {
        let prefix = format!("{open}x^2{}", &close[..close.len() - 1]);
        let suffix = format!("{open}x^2{close}");
        for (s, expected) in [(prefix.as_str(), false), (suffix.as_str(), true)] {
            let marked = metrics.mark(s, CELL, 80, Color::from_u32(0xffffff));
            assert_eq!(marked.text != s, expected, "{s}");
        }
    }
    let s = "code `$x$`, price $10 and $20, [link $a$](path), \\$z\n\n```sh\necho $$\n```";
    assert_eq!(metrics.mark(s, CELL, 80, Color::from_u32(0xffffff)).text, s);
}

#[test]
fn multirow_inline_equations_share_a_baseline_and_copy_delimiters() {
    let mut m = Measurements::default();
    let source = r"Use $x^2$ and \(\frac{1}{\sqrt{x}}\) together.";
    let marked = m.mark(source, CELL, 90, Color::from_u32(0xd3c8c8));
    let lines = marked
        .expand(vec![HyperlinkLine::from(marked.text.to_string())])
        .unwrap();
    let baseline = lines.iter().find(|l| !l.line.spans.is_empty()).unwrap();
    let runs = baseline.math.as_deref().unwrap();
    assert_eq!(runs.len(), 2);
    assert!(lines.len() > 1);
    assert!(runs.iter().all(|r| r.formula.above == r.row));
    assert_eq!(
        copy_source(baseline.line.to_string(), &runs.iter().collect::<Vec<_>>()),
        source
    );
}

#[test]
fn list_continuations_keep_the_equation_inside_the_item() {
    use pulldown_cmark::Event;
    use pulldown_cmark::Parser;
    use pulldown_cmark::Tag;
    use pulldown_cmark::TagEnd;
    let mut metrics = Measurements::default();
    let source = "1. First item.\n   $$x+y$$\n   More about it.\n2. Second item.";
    let marked = metrics.mark(source, CELL, 60, Color::from_u32(0xffffff));
    let mut in_item = false;
    let mut equation = false;
    let mut continuation = false;
    for event in Parser::new(&marked.text) {
        match event {
            Event::Start(Tag::Item) => in_item = true,
            Event::End(TagEnd::Item) => in_item = false,
            Event::Text(text) if text.contains('\u{f0000}') => equation = in_item,
            Event::Text(text) if text.contains("More about it") => continuation = in_item,
            _ => {}
        }
    }
    assert!(equation && continuation);
}

#[test]
fn unsupported_and_oversize_formulas_leave_readable_source() {
    let mut metrics = Measurements::default();
    let long = format!("$$ {} $$", "x+".repeat(2100));
    for text in [r"\(\notARealMathCommand{x}\)", &long] {
        assert_eq!(
            metrics.mark(text, CELL, 80, Color::from_u32(0xffffff)).text,
            text
        );
    }
    let wide = r"\[\underbrace{a+b+c+d+e+f+g+h+i+j}_{\text{ten terms}}\]";
    assert_eq!(
        metrics.mark(wide, CELL, 2, Color::from_u32(0xffffff)).text,
        wide
    );
}
