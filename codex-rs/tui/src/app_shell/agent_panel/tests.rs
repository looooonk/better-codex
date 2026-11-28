use super::view::Layout;
use super::*;
use crate::app_shell::agent_activity::AgentActivityState;
use crate::app_shell::agent_trace::tests::sample_trace;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::style::Color;

#[tokio::test]
async fn encrypted_arrows_are_inert_and_crossings_belong_to_the_middle_lane() {
    let trace = sample_trace().await;
    let mut agents = AgentActivityState::default();
    for lane in trace.lanes.iter().skip(1) {
        agents.assign_color(&lane.target.thread_id);
    }
    let mut panel = AgentPanel::default();
    panel.trace = Arc::new(trace);
    let screen = Rect::new(0, 0, 65, 20);
    let mut buf = Buffer::empty(screen);
    render(&panel, &agents, screen, None, &mut buf);
    let layout = Layout::new(&panel, screen);
    let y = layout.geometry.body.y + 1;
    let symbol_at = |y, symbol: &str| {
        (0..screen.width)
            .find(|x| buf[(*x, y)].symbol() == symbol)
            .unwrap()
    };
    let encrypted = Position::new(symbol_at(y, "─"), y);
    assert_eq!(buf[encrypted].fg, Color::DarkGray);
    assert_eq!(layout.hit(&panel, encrypted), None);
    panel.activate(Hit::Message(0));
    assert!(panel.detail.is_none());
    panel.navigate(KeyCode::Down);
    assert_eq!(panel.message, Some(1));
    panel.navigate(KeyCode::Down);
    assert_eq!(panel.message, Some(2));
    panel.navigate(KeyCode::Up);
    assert_eq!(panel.message, Some(1));
    let y = y + 6;
    let crossing = Position::new(symbol_at(y, "╴") + 1, y);
    assert_eq!(buf[crossing].symbol(), "┃");
    assert_eq!(
        buf[crossing].fg,
        agents.color(&panel.trace.lanes[1].target.thread_id)
    );
    assert_eq!(layout.hit(&panel, crossing), Some(Hit::Lane(1)));
    let arrow = Position::new(symbol_at(y, "─"), y);
    assert_eq!(buf[arrow].fg, Color::White);
    assert_eq!(layout.hit(&panel, arrow), Some(Hit::Message(2)));
}

#[tokio::test]
async fn message_detail_preserves_markdown_math_and_link_targets_after_wrapping() {
    use crate::app_shell::agent_trace::Body;
    use crate::terminal_hyperlinks::ClickTarget;
    use ratatui::style::Modifier;

    let mut trace = sample_trace().await;
    trace.messages[1].body = Arc::new(Body::Text(
        "# Result

**Bold** *italic* `code` $x^2$ [reference](https://example.com/notes)"
            .into(),
    ));
    let agents = AgentActivityState::default();
    let mut panel = AgentPanel::default();
    panel.trace = Arc::new(trace);
    panel.activate(Hit::Message(1));
    for width in [78, 35] {
        let screen = Rect::new(0, 0, width, 24);
        let mut buf = Buffer::empty(screen);
        render(&panel, &agents, screen, None, &mut buf);
        let body = Layout::new(&panel, screen).geometry.body;
        let find = |word: &str| {
            for y in body.y..body.bottom() {
                let row = (body.x..body.right())
                    .map(|x| crate::terminal_hyperlinks::strip_osc8(buf[(x, y)].symbol()))
                    .collect::<String>();
                if let Some(offset) = row.find(word) {
                    return Position::new(body.x + row[..offset].chars().count() as u16, y);
                }
            }
            panic!("missing {word:?} at width {width}");
        };
        assert!(buf[find("Result")].modifier.contains(Modifier::BOLD));
        assert!(buf[find("Bold")].modifier.contains(Modifier::BOLD));
        assert!(buf[find("italic")].modifier.contains(Modifier::ITALIC));
        assert_eq!(buf[find("x²")].symbol(), "x");
        assert_eq!(
            panel.link_at(find("reference")),
            Some(ClickTarget::External("https://example.com/notes".into()))
        );
    }
}
