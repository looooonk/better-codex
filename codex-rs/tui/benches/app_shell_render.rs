use divan::Bencher;

fn main() {
    divan::main();
}

#[divan::bench]
fn large_transcript_render(bencher: Bencher) {
    bencher.bench_local(|| {
        divan::black_box(codex_tui::app_shell_bench_support::render_large_transcript())
    });
}

#[divan::bench]
fn long_streaming_turn_render(bencher: Bencher) {
    bencher.bench_local(|| {
        divan::black_box(codex_tui::app_shell_bench_support::render_long_streaming_turn())
    });
}
