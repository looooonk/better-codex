use super::*;
use crate::equations::raster;
use ratatui::layout::Position;
use std::time::Duration;

async fn fixture(protocol: ImageProtocol) -> (Graphics, Arc<Formula>) {
    let cell = CellSize {
        width: 9,
        height: 18,
    };
    let source = r"\[\frac{a}{b}\]";
    let image = Arc::new(
        Formula::measure(
            raster::metrics(source, true).unwrap(),
            source.into(),
            true,
            cell,
            80,
            raster::MathColors {
                foreground: [200; 3],
                background: [0; 3],
            },
        )
        .unwrap(),
    );
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut graphics = Graphics::default();
    let mut cache =
        RasterCache::start(protocol == ImageProtocol::Sixel, FrameRequester::new(tx)).unwrap();
    assert!(cache.get(&image).is_none());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            cache.drain();
            if cache.get(&image).is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    graphics.worker = Some(cache);
    graphics.protocol = Some(protocol);
    (graphics, image)
}

fn frame(
    graphics: &mut Graphics,
    image: &Arc<Formula>,
    size: Rect,
    origin: Position,
    occluded: Option<Position>,
) -> (Buffer, Vec<u8>) {
    let mut buf = Buffer::empty(size);
    graphics.begin(size);
    if graphics.enabled() {
        for row in 0..image.rows() {
            let y = origin.y + row;
            if y >= size.bottom() {
                break;
            }
            let run = MathRun {
                formula: image.clone(),
                x: origin.x,
                row,
                marker: 'x',
            };
            graphics.mark(&mut buf, Rect::new(0, y, size.width, 1), &[run]);
        }
        if let Some(pos) = occluded {
            buf[pos].set_symbol("!");
        }
    }
    graphics.compose(&mut buf);
    let mut wire = Vec::new();
    graphics.present(&mut wire).unwrap();
    assert!(buf.content.iter().all(|c| c.symbol() != MASK));
    (buf, wire)
}

#[tokio::test]
async fn scrolling_resizing_and_occlusion_reuse_uploads_and_clear_obsolete_placements() {
    let (mut graphics, image) = fixture(ImageProtocol::Kitty).await;
    let area = Rect::new(0, 0, 64, 25);
    let a = Position::new(3, 5);
    let (_, wire) = frame(&mut graphics, &image, area, a, None);
    let text = String::from_utf8(wire).unwrap();
    assert!(text.contains("a=t,") && text.contains("a=p,"));
    assert!(frame(&mut graphics, &image, area, a, None).1.is_empty());

    let b = Position::new(4, 6);
    let mask = Position::new(b.x + 1, b.y);
    let (buf, wire) = frame(
        &mut graphics,
        &image,
        Rect::new(0, 0, 60, 22),
        b,
        Some(mask),
    );
    let text = String::from_utf8(wire).unwrap();
    assert_eq!(buf[mask].symbol(), "!");
    assert!(text.contains("a=d,d=i,") && text.contains("a=p,"));
    assert!(!text.contains("a=t,"));
    assert!(
        graphics
            .painted
            .iter()
            .all(|p| !p.placement.screen.contains(mask))
    );
    assert_eq!(
        graphics
            .painted
            .iter()
            .map(|p| p.placement.screen.area())
            .sum::<u32>(),
        u32::from(image.columns) * u32::from(image.rows()) - 1
    );
    graphics.disable();
    let (_, wire) = frame(&mut graphics, &image, area, a, None);
    assert!(String::from_utf8(wire).unwrap().contains("a=d,d=I,"));
    assert!(graphics.textures.is_none());
}

#[tokio::test]
async fn sixel_moves_restore_old_cells_and_crop_at_viewport_and_modal_edges() {
    let (mut graphics, image) = fixture(ImageProtocol::Sixel).await;
    let area = Rect::new(0, 0, 32, 9);
    let a = Position::new(5, 3);
    let (_, wire) = frame(&mut graphics, &image, area, a, None);
    assert!(wire.windows(2).any(|v| v == b"\x1bP"));
    let (stationary, wire) = frame(&mut graphics, &image, area, a, None);
    assert!(wire.is_empty());
    assert_eq!(stationary[a].symbol(), "\u{00a0}");
    assert_eq!(stationary[a].diff_option, CellDiffOption::Skip);
    let b = Position::new(5, 8);
    let (buf, _) = frame(&mut graphics, &image, area, b, Some(b));
    assert_eq!(buf[a].diff_option, CellDiffOption::AlwaysUpdate);
    assert_eq!(graphics.painted.len(), 1);
    assert_eq!(
        graphics.painted[0].placement.screen,
        Rect::new(6, 8, image.columns - 1, 1)
    );
}
