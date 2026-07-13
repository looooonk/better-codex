use divan::Bencher;

fn main() {
    divan::main();
}

#[divan::bench]
fn large_transcript_render(bencher: Bencher) {
    let fixture = codex_tui::app_shell_bench_support::long_history_fixture();
    divan::black_box(fixture.render());
    bencher.bench_local(|| divan::black_box(fixture.render()));
}

#[divan::bench]
fn large_transcript_scrolled_render(bencher: Bencher) {
    let mut fixture = codex_tui::app_shell_bench_support::long_history_fixture();
    divan::black_box(fixture.render());
    bencher.bench_local(|| divan::black_box(fixture.scroll_and_render()));
}

#[divan::bench]
fn long_streaming_turn_render(bencher: Bencher) {
    let fixture = codex_tui::app_shell_bench_support::long_streaming_turn_fixture();
    divan::black_box(fixture.render());
    bencher.bench_local(|| divan::black_box(fixture.render()));
}

#[divan::bench]
fn active_tool_output_update_and_render(bencher: Bencher) {
    let mut fixture = codex_tui::app_shell_bench_support::active_tool_output_fixture();
    divan::black_box(fixture.render());
    bencher.bench_local(|| divan::black_box(fixture.append_tool_output_and_render()));
}

#[divan::bench]
fn scroll_during_active_tool_output(bencher: Bencher) {
    let mut fixture = codex_tui::app_shell_bench_support::active_tool_output_fixture();
    divan::black_box(fixture.render());
    bencher.bench_local(|| divan::black_box(fixture.append_tool_output_scroll_and_render()));
}
