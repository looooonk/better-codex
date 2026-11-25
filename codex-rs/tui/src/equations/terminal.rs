//! Compose image placements from the final viewport cells, including modal occlusion.
//! Kitty separates image storage from placement. Moving a warm formula sends coordinates.
//! Sixel retains the encoded cell rows and repaints changed pixels inside the frame transaction.
use super::CellSize;
use super::Formula;
use super::MathRun;
use super::worker::Payload;
use super::worker::RasterCache;
use crate::pets::ImageProtocol;
use crate::tui::FrameRequester;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use lru::LruCache;
use ratatui::buffer::Buffer;
use ratatui::buffer::CellDiffOption;
use ratatui::layout::Rect;
use std::collections::HashMap;
use std::io::Write;
use std::io::{self};
use std::sync::Arc;

// Placed only by the transcript widget, consumed before the Ratatui diff.
const MASK: &str = "\u{e000}\u{300}";
const DEVICE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct Placement {
    image: Arc<Formula>,
    screen: Rect,
    left: u16,
    top: u16,
}

struct Candidate {
    run: MathRun,
    x: u16,
    y: u16,
    width: u16,
}
struct Texture {
    id: u32,
    bytes: usize,
}
struct Painted {
    placement: Placement,
    image_id: u32,
    placement_id: u32,
}
#[derive(Default)]
pub(crate) struct Graphics {
    pub cell: Option<CellSize>,
    protocol: Option<ImageProtocol>,
    worker: Option<RasterCache>,
    frame: Option<FrameRequester>,
    candidates: Vec<Candidate>,
    next: Vec<Placement>,
    ready: HashMap<Arc<Formula>, Arc<Payload>>,
    painted: Vec<Painted>,
    textures: Option<LruCache<Arc<Formula>, Texture>>,
    bytes: usize,
    serial: u32,
    previous_size: Option<Rect>,
    resized: bool,
}

impl Graphics {
    pub fn configure(&mut self, frame: FrameRequester) {
        self.frame = Some(frame);
    }
    pub fn enabled(&self) -> bool {
        self.worker.is_some()
    }
    pub fn enable(&mut self) -> Result<(), String> {
        let support = crate::pets::detect_pet_image_support();
        let protocol = support.protocol().ok_or_else(|| {
            "This terminal does not expose supported Kitty or Sixel graphics to Better Codex."
                .to_owned()
        })?;
        if !crate::terminal_probe::EQUATION_GRAPHICS_CONFIRMED
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Err("The terminal did not acknowledge the graphics probe. Equation images require Kitty graphics or Sixel.".into());
        }
        let cell = CellSize::detect()
            .ok_or("The terminal must report pixel dimensions for equation layout.")?;
        let frame = self
            .frame
            .clone()
            .ok_or("Terminal graphics are unavailable in this view.")?;
        self.worker = Some(
            RasterCache::start(protocol == ImageProtocol::Sixel, frame)
                .map_err(|e| e.to_string())?,
        );
        self.protocol = Some(protocol);
        self.cell = Some(cell);
        Ok(())
    }
    pub fn disable(&mut self) {
        self.worker = None;
        self.cell = None;
    }
    pub fn stream_delta(&self, text: &str) {
        if self.enabled()
            && text.contains(['$', ')', ']'])
            && let Some(frame) = &self.frame
        {
            frame.schedule_frame();
        }
    }
    /// A font zoom can change cell metrics independently of the terminal dimensions.
    pub fn refresh_cell(&mut self) {
        if self.enabled() {
            self.cell = CellSize::detect();
        }
    }
    pub fn viewport_cleared(&mut self) {
        self.previous_size = None;
    }
    pub fn begin(&mut self, size: Rect) {
        self.resized = self.previous_size.replace(size) != Some(size);
        self.candidates.clear();
        self.next.clear();
        self.ready.clear();
        if let Some(cache) = &mut self.worker {
            cache.drain();
        }
    }
    pub fn mark(&mut self, buf: &mut Buffer, row: Rect, runs: &[MathRun]) {
        for run in runs {
            let x = row.x.saturating_add(run.x);
            let width = run.formula.columns.min(row.right().saturating_sub(x));
            for col in x..x + width {
                buf[(col, row.y)].set_symbol(MASK);
            }
            if width > 0 {
                // Let the worker rasterize while the dashboard and remaining widgets draw.
                if self
                    .textures
                    .as_ref()
                    .is_none_or(|textures| !textures.contains(&run.formula))
                    && let Some(cache) = self.worker.as_mut()
                {
                    cache.request(&run.formula);
                }
                self.candidates.push(Candidate {
                    run: run.clone(),
                    x,
                    y: row.y,
                    width,
                });
            }
        }
    }
    pub fn compose(&mut self, buf: &mut Buffer) {
        // A small equation can finish during this same frame.
        if let Some(cache) = self.worker.as_mut() {
            cache.drain();
        }
        // Consecutive exposed slices form rectangles; covered cells are never painted over.
        let mut last = HashMap::new();
        for candidate in self.candidates.drain(..) {
            let Candidate { run, x, y, width } = candidate;
            let available = if self
                .textures
                .as_mut()
                .and_then(|c| c.get(&run.formula))
                .is_some()
                || self.ready.contains_key(&run.formula)
            {
                true
            } else if let Some(p) = self.worker.as_mut().and_then(|c| c.get(&run.formula)) {
                self.ready.insert(run.formula.clone(), p);
                true
            } else {
                false
            };
            let mut col = 0;
            while col < width {
                if buf[(x + col, y)].symbol() != MASK {
                    col += 1;
                    continue;
                }
                let left = col;
                while col < width && buf[(x + col, y)].symbol() == MASK {
                    buf[(x + col, y)].set_symbol(" ");
                    col += 1;
                }
                if !available {
                    if run.row == run.formula.above {
                        let source = run.formula.source.replace('\n', " ");
                        let excerpt = source.chars().skip(usize::from(left)).collect::<String>();
                        let style = buf[(x + left, y)].style();
                        buf.set_stringn(x + left, y, excerpt, usize::from(col - left), style);
                    }
                    continue;
                }
                let key = (
                    run.formula.clone(),
                    x,
                    i32::from(y) - i32::from(run.row),
                    left,
                    col,
                );
                if let Some(&index) = last.get(&key) {
                    let p: &mut Placement = &mut self.next[index];
                    if p.screen.bottom() == y {
                        p.screen.height += 1;
                        continue;
                    }
                }
                last.insert(key, self.next.len());
                self.next.push(Placement {
                    image: run.formula.clone(),
                    screen: Rect::new(x + left, y, col - left, 1),
                    left,
                    top: run.row,
                });
            }
        }
        if self.protocol == Some(ImageProtocol::Sixel) {
            let changed = self.changed();
            // An ordinary space can be erased by the terminal's clear-to-end-of-line
            // optimization. Keep Sixel cells in the drawable prefix on all frames.
            for placement in &self.next {
                for row in placement.screen.rows() {
                    for x in row.x..row.right() {
                        let cell = &mut buf[(x, row.y)];
                        cell.set_symbol("\u{00a0}");
                        if !changed {
                            cell.diff_option = CellDiffOption::Skip;
                        }
                    }
                }
            }
            if !changed {
                return;
            }
            for p in &self.painted {
                for row in p.placement.screen.intersection(buf.area).rows() {
                    for x in row.x..row.right() {
                        buf[(x, row.y)].diff_option = CellDiffOption::AlwaysUpdate;
                    }
                }
            }
        }
    }
    fn changed(&self) -> bool {
        self.resized || !self.painted.iter().map(|p| &p.placement).eq(&self.next)
    }
    fn id(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1).max(1);
        0x4100_0000 | (self.serial & 0x00ff_ffff)
    }
    pub fn present(&mut self, w: &mut impl Write) -> io::Result<()> {
        if self.painted.is_empty()
            && self.next.is_empty()
            && (self.enabled() || self.textures.is_none())
        {
            return Ok(());
        }
        if self.changed() || !self.enabled() {
            if self.protocol == Some(ImageProtocol::Sixel) {
                self.sixel(w)?;
            } else {
                self.kitty(w)?;
            }
        }
        Ok(())
    }
    fn sixel(&mut self, w: &mut impl Write) -> io::Result<()> {
        self.painted.clear();
        if !self.next.is_empty() {
            w.write_all(b"\x1b7")?;
        }
        for p in &self.next {
            let Some(Payload::Sixel { pixels, full, rows }) =
                self.ready.get(&p.image).map(Arc::as_ref)
            else {
                continue;
            };
            let cw = u32::from(p.image.cell.width);
            let ch = u32::from(p.image.cell.height);
            if p.screen.width == p.image.columns && p.screen.height == p.image.rows() {
                write!(w, "\x1b[{};{}H", p.screen.y + 1, p.screen.x + 1)?;
                w.write_all(full)?;
            } else {
                for offset in 0..p.screen.height {
                    let y = p.top + offset;
                    write!(w, "\x1b[{};{}H", p.screen.y + offset + 1, p.screen.x + 1)?;
                    if p.left == 0 && p.screen.width == p.image.columns {
                        w.write_all(&rows[usize::from(y)])?;
                    } else {
                        let crop = image::imageops::crop_imm(
                            pixels,
                            u32::from(p.left) * cw,
                            u32::from(y) * ch,
                            u32::from(p.screen.width) * cw,
                            ch,
                        )
                        .to_image();
                        let bytes = crate::pets::sixel::encode_rgba(
                            crop.as_raw(),
                            crop.width(),
                            crop.height(),
                        )
                        .map_err(io::Error::other)?;
                        w.write_all(&bytes)?;
                    }
                }
            }
            self.painted.push(Painted {
                placement: p.clone(),
                image_id: 0,
                placement_id: 0,
            });
        }
        if !self.next.is_empty() {
            w.write_all(b"\x1b8")?;
        }
        Ok(())
    }
    fn kitty(&mut self, w: &mut impl Write) -> io::Result<()> {
        let mut retained = HashMap::new();
        for p in self.painted.drain(..) {
            if !self.resized && self.next.contains(&p.placement) {
                retained.insert(p.placement.clone(), p);
            } else {
                write!(
                    w,
                    "\x1b_Ga=d,d=i,i={},p={},q=2;\x1b\\",
                    p.image_id, p.placement_id
                )?;
            }
        }
        let mut textures = self.textures.take().unwrap_or_else(LruCache::unbounded);
        // A viewport clear invalidates visible placements. Transmitted image data is
        // retained independently by Kitty, so resize reuses the existing texture IDs.
        let mut moved = false;
        for index in 0..self.next.len() {
            let p = self.next[index].clone();
            if let Some(previous) = retained.remove(&p) {
                self.painted.push(previous);
                continue;
            }
            let image_id = if let Some(t) = textures.get(&p.image) {
                t.id
            } else {
                let id = self.id();
                let Some(Payload::Kitty(bytes)) = self.ready.get(&p.image).map(Arc::as_ref) else {
                    continue;
                };
                upload(w, id, bytes)?;
                let bytes = usize::from(p.image.columns)
                    * usize::from(p.image.cell.width)
                    * usize::from(p.image.rows())
                    * usize::from(p.image.cell.height)
                    * 4;
                textures.put(p.image.clone(), Texture { id, bytes });
                self.bytes += bytes;
                id
            };
            if !moved {
                w.write_all(b"\x1b7")?;
                moved = true;
            }
            let pid = self.id();
            let cw = u32::from(p.image.cell.width);
            let ch = u32::from(p.image.cell.height);
            write!(
                w,
                "\x1b[{};{}H\x1b_Ga=p,i={image_id},p={pid},x={},y={},w={},h={},c={},r={},C=1,q=2;\x1b\\",
                p.screen.y + 1,
                p.screen.x + 1,
                u32::from(p.left) * cw,
                u32::from(p.top) * ch,
                u32::from(p.screen.width) * cw,
                u32::from(p.screen.height) * ch,
                p.screen.width,
                p.screen.height
            )?;
            self.painted.push(Painted {
                placement: p,
                image_id,
                placement_id: pid,
            });
        }
        if moved {
            w.write_all(b"\x1b8")?;
        }
        let count = textures.len();
        for _ in 0..count {
            if self.enabled() && textures.len() <= 192 && self.bytes <= DEVICE_BYTES {
                break;
            }
            let Some((f, t)) = textures.pop_lru() else {
                break;
            };
            if self.painted.iter().any(|p| p.image_id == t.id) {
                textures.put(f, t);
                continue;
            }
            self.bytes -= t.bytes;
            delete(w, t.id)?;
        }
        self.textures = (!textures.is_empty()).then_some(textures);
        Ok(())
    }
}
impl Drop for Graphics {
    fn drop(&mut self) {
        if let Some(textures) = &self.textures {
            let mut out = io::stdout().lock();
            for (_, texture) in textures.iter() {
                let _ = delete(&mut out, texture.id);
            }
            let _ = out.flush();
        }
    }
}

fn delete(w: &mut impl Write, id: u32) -> io::Result<()> {
    write!(w, "\x1b_Ga=d,d=I,i={id},q=2;\x1b\\")
}
fn upload(w: &mut impl Write, id: u32, bytes: &[u8]) -> io::Result<()> {
    let data = STANDARD.encode(bytes);
    let mut chunks = data.as_bytes().chunks(4096).peekable();
    let mut first = true;
    while let Some(chunk) = chunks.next() {
        let more = u8::from(chunks.peek().is_some());
        if first {
            write!(w, "\x1b_Ga=t,t=d,f=100,i={id},q=2,m={more};")?;
            first = false;
        } else {
            write!(w, "\x1b_Gm={more};")?;
        }
        w.write_all(chunk)?;
        w.write_all(b"\x1b\\")?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "terminal_tests.rs"]
mod tests;
