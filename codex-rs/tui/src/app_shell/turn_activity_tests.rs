use super::*;
use pretty_assertions::assert_eq;

#[test]
fn white_highlight_moves_right_across_gray_activity_and_rate() {
    let area = Rect::new(
        /*x*/ 2, /*y*/ 1, /*width*/ 40, /*height*/ 1,
    );
    let frame = |elapsed| {
        let mut buf = Buffer::empty(area);
        render_activity("thinking", /*rate*/ 32, elapsed, area, &mut buf);
        buf
    };
    let static_text = frame(None);
    let early = frame(Some(Duration::from_millis(/*millis*/ 400)));
    let late = frame(Some(Duration::from_millis(/*millis*/ 1400)));
    let start = area.x + 1;
    let colors = |buf: &Buffer| [2, 17].map(|offset| buf[(start + offset, area.y)].fg);
    let gray = rgb_color((152, 152, 152));
    let white = rgb_color((248, 248, 248));
    assert_eq!(
        [colors(&static_text), colors(&early), colors(&late)],
        [[gray, gray], [white, gray], [gray, white]]
    );
    assert_eq!(
        early
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>()
            .trim(),
        "Thinking... · 32 tok/s"
    );
}
