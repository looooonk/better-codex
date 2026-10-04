use super::*;
use crate::pets::ImageProtocol;

#[test]
fn terminal_handoff_erases_the_owned_sprite_and_redraw_can_resume() {
    let dir = tempfile::tempdir().unwrap();
    let frame = dir.path().join("frame.png");
    std::fs::write(&frame, b"png").unwrap();
    let request = AmbientPetDraw {
        frame,
        protocol: ImageProtocol::Kitty,
        x: 40,
        y: 2,
        clear_top_y: 0,
        columns: 5,
        rows: 5,
        height_px: 75,
        sixel_dir: dir.path().to_path_buf(),
    };
    let state = TerminalPetImage::default();
    let mut output = Vec::new();
    state.draw(&mut output, Some(request.clone())).unwrap();
    output.clear();
    clear_active_terminal_image(&mut output).unwrap();
    assert!(String::from_utf8_lossy(&output).contains("Ga=d,d=I,i=49374,q=2;"));
    output.clear();
    clear_active_terminal_image(&mut output).unwrap();
    assert!(output.is_empty());
    state.draw(&mut output, Some(request)).unwrap();
    assert!(String::from_utf8_lossy(&output).contains("cG5n"));
    clear_active_terminal_image(&mut output).unwrap();
}
