use super::*;
use pretty_assertions::assert_eq;

#[test]
fn stops_and_hit_targets_share_the_centers_of_the_caps() {
    let panel = Rect::new(
        /*x*/ 5, /*y*/ 3, /*width*/ 40, /*height*/ 7,
    );
    let rail = EffortRail::new(panel, /*stops*/ 5);
    let mut buf = Buffer::empty(panel);
    rail.render(/*selected*/ 3, /*rainbow_frame*/ None, &mut buf);
    let middle = rail.area.y + 1;
    let markers: Vec<_> = (rail.area.x..rail.area.right())
        .filter(|&x| matches!(buf[(x, middle)].symbol(), "•" | "●"))
        .map(|x| (x, rail.nearest_stop(x)))
        .collect();
    assert_eq!(markers, [(9, 1), (17, 2), (25, 3), (32, 4), (40, 5)]);
    assert_eq!(buf[(25, middle)].symbol(), "●");

    // The fill continues past the thumb and tapers into the unfilled track.
    let thumb = markers[2].0;
    let filled = buf[(thumb, middle)].bg;
    let edge = buf[(thumb + 2, middle)].bg;
    let empty = buf[(thumb + 3, middle)].bg;
    assert_ne!(edge, filled);
    assert_ne!(edge, empty);
    assert_ne!(buf[(thumb + 2, rail.area.y)].fg, edge);
    assert_eq!(
        buf[(thumb + 2, rail.area.y)].fg,
        buf[(thumb + 2, middle + 1)].fg
    );

    // At the lowest stop the thumb sits two columns from the left edge.
    rail.render(/*selected*/ 1, /*rainbow_frame*/ None, &mut buf);
    assert_eq!(buf[(rail.area.x + 2, middle)].symbol(), "●");
}
