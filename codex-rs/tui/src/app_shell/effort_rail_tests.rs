use super::*;
use pretty_assertions::assert_eq;

#[test]
fn stage_ticks_match_the_hit_targets_and_the_end_of_the_fill() {
    let panel = Rect::new(
        /*x*/ 5, /*y*/ 3, /*width*/ 40, /*height*/ 6,
    );
    let rail = EffortRail::new(panel, /*stops*/ 5);
    let mut buf = Buffer::empty(panel);
    let bar_y = rail.area.y;
    let stops = [7, 16, 25, 33, 42];

    for (index, &selected_x) in stops.iter().enumerate() {
        rail.render(index + 1, /*rainbow_frame*/ None, &mut buf);
        for x in rail.area.x..rail.area.right() {
            let color = if x <= selected_x {
                palette::purple()
            } else {
                palette::border()
            };
            let cell = &buf[(x, bar_y)];
            assert_eq!((cell.symbol(), cell.fg, cell.bg), ("█", color, color));
            let tick = if x == selected_x {
                "▴"
            } else if stops.contains(&x) {
                "╵"
            } else {
                " "
            };
            assert_eq!(buf[(x, bar_y + 1)].symbol(), tick);
        }
        assert_eq!(rail.nearest_stop(selected_x), index + 1);
    }
}

#[test]
fn a_single_supported_stage_fills_its_track_and_has_one_centered_tick() {
    let panel = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 40, /*height*/ 6,
    );
    let rail = EffortRail::new(panel, /*stops*/ 1);
    let mut buf = Buffer::empty(panel);
    rail.render(/*selected*/ 1, /*rainbow_frame*/ None, &mut buf);
    let middle = rail.area.x + rail.area.width / 2;
    for x in rail.area.x..rail.area.right() {
        assert_eq!(buf[(x, rail.area.y)].bg, palette::purple());
        assert_eq!(
            buf[(x, rail.area.y + 1)].symbol(),
            if x == middle { "▴" } else { " " }
        );
        assert_eq!(rail.nearest_stop(x), 1);
    }
}
