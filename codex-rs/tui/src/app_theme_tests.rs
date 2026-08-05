use super::*;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

#[derive(Debug)]
#[allow(dead_code, reason = "fields are read through Debug by insta snapshots")]
struct RenderedCell {
    symbol: String,
    foreground: Color,
    background: Color,
    modifiers: Modifier,
}

fn render_theme_sample(theme: TuiAppTheme) -> Vec<RenderedCell> {
    let _active_theme = activate(theme);
    let palette = palette();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 7, /*height*/ 1,
    );
    let mut buffer = Buffer::empty(area);
    let spans = [
        ("B", palette.text, palette.base),
        ("D", palette.muted, palette.dark),
        ("S", palette.cyan, palette.surface),
        ("E", palette.purple, palette.elevated),
        ("+", palette.success, palette.diff_added_background),
        ("!", palette.warning, palette.border),
        ("-", palette.error, palette.diff_removed_background),
    ]
    .into_iter()
    .map(|(symbol, foreground, background)| {
        Span::styled(symbol, Style::new().fg(foreground).bg(background).bold())
    })
    .collect::<Vec<_>>();
    Paragraph::new(Line::from(spans)).render(area, &mut buffer);
    buffer
        .content()
        .iter()
        .map(|cell| RenderedCell {
            symbol: cell.symbol().to_string(),
            foreground: cell.fg,
            background: cell.bg,
            modifiers: cell.modifier,
        })
        .collect()
}

#[test]
fn gruvbox_dark_renders_with_its_palette() {
    insta::assert_debug_snapshot!(render_theme_sample(TuiAppTheme::GruvboxDark));
}

#[test]
fn catppuccin_mocha_renders_with_its_palette() {
    insta::assert_debug_snapshot!(render_theme_sample(TuiAppTheme::CatppuccinMocha));
}

#[test]
fn monochrome_renders_with_its_palette() {
    insta::assert_debug_snapshot!(render_theme_sample(TuiAppTheme::Monochrome));
}

#[test]
fn nested_activation_restores_the_previous_palette() {
    let initial = palette();
    let gruvbox = {
        let _gruvbox = activate(TuiAppTheme::GruvboxDark);
        palette()
    };
    let catppuccin = {
        let _catppuccin = activate(TuiAppTheme::CatppuccinMocha);
        palette()
    };

    let _gruvbox = activate(TuiAppTheme::GruvboxDark);
    assert_eq!(palette(), gruvbox);
    {
        let _catppuccin = activate(TuiAppTheme::CatppuccinMocha);
        assert_eq!(palette(), catppuccin);
    }
    assert_eq!(palette(), gruvbox);
    drop(_gruvbox);
    assert_eq!(palette(), initial);
}
