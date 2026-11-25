//! A single rasterizer per enabled TUI. A saturated queue retries on the next completion.
use super::Formula;
use super::raster;
use crate::tui::FrameRequester;
use image::RgbaImage;
use lru::LruCache;
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

const CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_IN_FLIGHT: usize = 8;

pub(super) enum Payload {
    Kitty(Vec<u8>),
    Sixel {
        pixels: RgbaImage,
        full: Vec<u8>,
        rows: Vec<Vec<u8>>,
    },
}
impl Payload {
    fn render(formula: &Formula, sixel: bool) -> Option<Self> {
        let mut pixels = formula.rasterize()?;
        if !sixel {
            return Some(Self::Kitty(raster::png(pixels)?));
        }
        // Sixel has binary transparency. Blend fractional glyph coverage against
        // the pane while leaving fully transparent cells available to the terminal.
        for pixel in pixels.pixels_mut() {
            let alpha = u16::from(pixel[3]);
            if alpha > 0 && alpha < 255 {
                for (channel, bg) in pixel.0[..3].iter_mut().zip(formula.colors.background) {
                    *channel = ((u16::from(*channel) * alpha + u16::from(bg) * (255 - alpha) + 127)
                        / 255) as u8;
                }
                pixel[3] = 255;
            }
        }
        let full =
            crate::pets::sixel::encode_rgba(pixels.as_raw(), pixels.width(), pixels.height())
                .ok()?;
        let height = u32::from(formula.cell.height);
        let stride = usize::try_from(height * pixels.width() * 4).ok()?;
        let rows = pixels
            .as_raw()
            .chunks_exact(stride)
            .map(|row| crate::pets::sixel::encode_rgba(row, pixels.width(), height).ok())
            .collect::<Option<Vec<_>>>()?;
        Some(Self::Sixel { pixels, full, rows })
    }
    fn bytes(&self) -> usize {
        match self {
            Self::Kitty(bytes) => bytes.len(),
            Self::Sixel { pixels, full, rows } => {
                full.len() + pixels.as_raw().len() + rows.iter().map(Vec::len).sum::<usize>()
            }
        }
    }
}

type ResultEntry = (Arc<Formula>, Option<Arc<Payload>>);
pub(super) struct RasterCache {
    jobs: mpsc::SyncSender<Arc<Formula>>,
    results: mpsc::Receiver<ResultEntry>,
    pending: HashSet<Arc<Formula>>,
    entries: LruCache<Arc<Formula>, Option<Arc<Payload>>>,
    bytes: usize,
    cancelled: Arc<AtomicBool>,
}
impl RasterCache {
    pub fn start(sixel: bool, frame: FrameRequester) -> std::io::Result<Self> {
        let (jobs, rx) = mpsc::sync_channel::<Arc<Formula>>(MAX_IN_FLIGHT);
        let (tx, results) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        std::thread::Builder::new()
            .name("equation-rasterizer".into())
            .spawn(move || {
                while let Ok(f) = rx.recv() {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let image = Payload::render(&f, sixel).map(Arc::new);
                    if tx.send((f, image)).is_err() {
                        break;
                    }
                    frame.schedule_frame();
                }
            })?;
        Ok(Self {
            jobs,
            results,
            pending: HashSet::new(),
            entries: LruCache::new(const { NonZeroUsize::new(192).unwrap() }),
            bytes: 0,
            cancelled,
        })
    }

    pub fn drain(&mut self) {
        while let Ok((key, payload)) = self.results.try_recv() {
            self.pending.remove(&key);
            self.bytes += payload.as_ref().map_or(0, |p| p.bytes());
            if let Some((_, Some(evicted))) = self.entries.push(key, payload) {
                self.bytes -= evicted.bytes();
            }
            while self.bytes > CACHE_BYTES && self.entries.len() > 1 {
                if let Some((_, Some(evicted))) = self.entries.pop_lru() {
                    self.bytes -= evicted.bytes();
                }
            }
        }
    }

    pub fn get(&mut self, key: &Arc<Formula>) -> Option<Arc<Payload>> {
        if let Some(entry) = self.entries.get(key) {
            return entry.clone();
        }
        self.request(key);
        None
    }

    pub fn request(&mut self, key: &Arc<Formula>) {
        if self.pending.len() < MAX_IN_FLIGHT
            && !self.entries.contains(key)
            && !self.pending.contains(key)
            && self.jobs.try_send(key.clone()).is_ok()
        {
            self.pending.insert(key.clone());
        }
    }
}
impl Drop for RasterCache {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
