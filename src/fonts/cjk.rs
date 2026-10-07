//! Owned SD font preparation. Layout and draw borrow only prepared RAM data.
use super::{FontSet, Style, StyleState, bitmap::BitmapFont};
use crate::{
    drivers::strip::StripBuffer,
    error::{Error, ErrorKind, Result},
    kernel::{BigBuf, BufClass, FONT_GLYPHS_PSRAM_BYTES, KernelHandle},
};
use alloc::{boxed::Box, vec::Vec};
use core::mem::size_of;
use embedded_graphics::pixelcolor::BinaryColor;
use pulp_fontpack::{
    FontError, Glyph, Metrics, PACK_DIR, PackReader, PageCache, PageGlyphSlot, ReadAt, bitmap_size,
    missing_glyph_metrics, pack_file_name,
};
use smol_epub::html_strip::{IMG_REF, MARKER};

pub const BODY_PIXELS: [u16; 5] = [16, 19, 23, 28, 35];
pub const HEADING_PIXELS: [u16; 5] = [23, 27, 32, 38, 46];
// Body state: persistent internal metrics <=16 KiB (growth can temporarily
// hold old and new vectors, <=32 KiB), two slot tables <=16 KiB each, scratch
// <=4 KiB. Bitmap bytes use the explicit FontGlyphs board budget, in PSRAM on
// C61.
// Auxiliary surfaces are body role only: <=4 KiB metrics, 170 slots (~4 KiB)
// and <=32 KiB bitmaps each. Alive together with the reader are its title, its
// TOC and the manager overlay; Files, Home and Settings clear their surfaces
// on exit and suspend, so they never add to those three.
pub const CJK_STORAGE_BUDGET: usize = 336 * 1024;
const METRIC_BUDGET: usize = 16 * 1024;
const SLOT_BUDGET: usize = 16 * 1024;
const BITMAP_BUDGET: usize = 64 * 1024;
const CACHE_BUDGET: usize = SLOT_BUDGET + BITMAP_BUDGET + size_of::<OwnedCache>();
const SCRATCH_BUDGET: usize = 4 * 1024;
const AUX_SURFACES: usize = 3;
const AUX_METRIC_BUDGET: usize = 4 * 1024;
// 170 slots is what 4 KiB holds with the 24-byte slot of the 32-bit targets;
// counting slots gives the 64-bit host the same ceiling.
const AUX_SLOT_BUDGET: usize = 170 * size_of::<PageGlyphSlot>();
const AUX_BITMAP_BUDGET: usize = 32 * 1024;
const AUX_CACHE_BUDGET: usize = AUX_SLOT_BUDGET + AUX_BITMAP_BUDGET + size_of::<OwnedCache>();
type OwnedCache = PageCache<Box<[PageGlyphSlot]>, BigBuf>;
const _: () = assert!(
    2 * METRIC_BUDGET
        + 2 * CACHE_BUDGET
        + SCRATCH_BUDGET
        + size_of::<CjkState>()
        + AUX_SURFACES * (AUX_METRIC_BUDGET + AUX_CACHE_BUDGET + size_of::<CjkState>())
        <= CJK_STORAGE_BUDGET
);
const _: () =
    assert!(2 * BITMAP_BUDGET + AUX_SURFACES * AUX_BITMAP_BUDGET <= FONT_GLYPHS_PSRAM_BYTES);

fn failure(kind: ErrorKind) -> Error {
    Error::from_kind(kind).with_source("font")
}
fn font_error(e: FontError<Error>) -> Error {
    match e {
        FontError::Io(e) => e.with_source("font"),
        _ => failure(ErrorKind::InvalidData),
    }
}
fn buffer<T: Clone>(len: usize, value: T, budget: usize) -> Result<Box<[T]>> {
    if len.checked_mul(size_of::<T>()).is_none_or(|n| n > budget) {
        return Err(failure(ErrorKind::BufferTooSmall));
    }
    let mut v = Vec::new();
    v.try_reserve_exact(len)
        .map_err(|_| failure(ErrorKind::OutOfMemory))?;
    if v.capacity() * size_of::<T>() > budget {
        return Err(failure(ErrorKind::BufferTooSmall));
    }
    v.resize(len, value);
    Ok(v.into_boxed_slice())
}
enum Source<'a, 'k> {
    Installed {
        handle: &'a mut KernelHandle<'k>,
        name: pulp_fontpack::PackFileName,
    },
    // A validated zero-glyph pack drives the same pure missing-glyph cache
    // preparation as an installed pack that lacks a requested scalar.
    Empty([u8; pulp_fontpack::HEADER_LEN]),
}
impl ReadAt for Source<'_, '_> {
    type Error = Error;
    fn read_at(&mut self, offset: u64, mut buf: &mut [u8]) -> Result<()> {
        match self {
            Self::Empty(header) => {
                let mut bytes = &header[..];
                bytes
                    .read_at(offset, buf)
                    .map_err(|_| failure(ErrorKind::InvalidData))
            }
            Self::Installed { handle, name } => {
                let mut at = u32::try_from(offset).map_err(|_| failure(ErrorKind::InvalidData))?;
                while !buf.is_empty() {
                    let n = handle.read_app_subdir_chunk(PACK_DIR, name.as_str(), at, buf)?;
                    if n == 0 || n > buf.len() {
                        return Err(failure(ErrorKind::ReadFailed));
                    }
                    at = at
                        .checked_add(n as u32)
                        .ok_or(failure(ErrorKind::InvalidData))?;
                    buf = &mut buf[n..];
                }
                Ok(())
            }
        }
    }
}
fn empty_header(px: u16) -> [u8; pulp_fontpack::HEADER_LEN] {
    let mut raw = [0; pulp_fontpack::HEADER_LEN];
    raw[..4].copy_from_slice(&pulp_fontpack::MAGIC);
    raw[4..6].copy_from_slice(&pulp_fontpack::FORMAT_VERSION.to_le_bytes());
    raw[6..8].copy_from_slice(&px.to_le_bytes());
    raw[16..18].copy_from_slice(&px.to_le_bytes());
    raw[18..20].copy_from_slice(&px.to_le_bytes());
    // With zero records and zero bitmap bytes, index, bitmap and EOF coincide.
    for offset in [24, 32, 40] {
        raw[offset..offset + 4].copy_from_slice(&(pulp_fontpack::HEADER_LEN as u32).to_le_bytes());
    }
    raw
}
fn open<'a, 'k>(k: &'a mut KernelHandle<'k>, px: u16) -> Result<PackReader<Source<'a, 'k>>> {
    let name = pack_file_name(px);
    let reader = match k.optional_file_size_app_subdir(PACK_DIR, name.as_str()) {
        Ok(Some(len)) => PackReader::open(Source::Installed { handle: k, name }, len.into())
            .map_err(font_error)?,
        Ok(None) => PackReader::open(
            Source::Empty(empty_header(px)),
            pulp_fontpack::HEADER_LEN as u64,
        )
        .map_err(font_error)?,
        Err(e) => return Err(e.with_source("font")),
    };
    if reader.info().pixel_size != px {
        return Err(failure(ErrorKind::InvalidData));
    }
    Ok(reader)
}
/// Validated installed bank identity; an absent optional bank has font ID zero.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BankIdentity {
    pub pixel_size: u16,
    pub font_id: u64,
    pub installed: bool,
}

pub fn bank_identity(k: &mut KernelHandle<'_>, px: u16) -> Result<BankIdentity> {
    let installed = k
        .optional_file_size_app_subdir(PACK_DIR, pack_file_name(px).as_str())
        .map_err(|e| e.with_source("font"))?
        .is_some();
    let reader = open(k, px)?;
    Ok(BankIdentity {
        pixel_size: px,
        font_id: reader.info().font_id,
        installed,
    })
}

#[derive(Clone, Copy)]
struct Entry {
    px: u16,
    ch: char,
    metrics: Option<Metrics>,
    visible: bool,
}

pub struct CjkState {
    metrics: Vec<Entry>,
    body: Option<(u16, OwnedCache)>,
    heading: Option<(u16, OwnedCache)>,
    heading_role: bool,
    metric_budget: usize,
    slot_budget: usize,
    bitmap_budget: usize,
}
impl CjkState {
    pub const fn new() -> Self {
        Self {
            metrics: Vec::new(),
            body: None,
            heading: None,
            heading_role: true,
            metric_budget: METRIC_BUDGET,
            slot_budget: SLOT_BUDGET,
            bitmap_budget: BITMAP_BUDGET,
        }
    }
    /// Auxiliary visible labels have smaller, independent allocation ceilings
    /// and prepare the body role only.
    pub const fn auxiliary() -> Self {
        Self {
            metrics: Vec::new(),
            body: None,
            heading: None,
            heading_role: false,
            metric_budget: AUX_METRIC_BUDGET,
            slot_budget: AUX_SLOT_BUDGET,
            bitmap_budget: AUX_BITMAP_BUDGET,
        }
    }
    pub fn clear(&mut self) {
        self.metrics = Vec::new();
        self.body = None;
        self.heading = None;
    }
    pub fn stage_metrics(
        &mut self,
        k: &mut KernelHandle<'_>,
        text: &[u8],
        latin: FontSet,
        body_px: u16,
        heading_px: u16,
    ) -> Result<()> {
        self.stage_metrics_with_flags(k, text, latin, body_px, heading_px, 0)
    }
    /// Stage a window whose opening style markers occurred on an earlier page.
    /// Bits 0, 1 and 2 are bold, italic and heading respectively.
    pub fn stage_metrics_with_flags(
        &mut self,
        k: &mut KernelHandle<'_>,
        text: &[u8],
        latin: FontSet,
        body_px: u16,
        heading_px: u16,
        initial_flags: u8,
    ) -> Result<()> {
        self.metrics.clear();
        let mut collection = Ok(());
        scan(text, initial_flags, |ch, sty| {
            if collection.is_err() || !needs(latin, ch, sty) {
                return;
            }
            let key = (pixel_size(sty, body_px, heading_px), ch);
            if let Err(index) = self.metrics.binary_search_by_key(&key, |e| (e.px, e.ch)) {
                if (self.metrics.len() + 1) * size_of::<Entry>() > self.metric_budget {
                    collection = Err(failure(ErrorKind::BufferTooSmall));
                    return;
                }
                if self.metrics.len() == self.metrics.capacity() {
                    let ceiling = self.metric_budget / size_of::<Entry>();
                    let capacity = self.metrics.capacity();
                    let target = capacity.saturating_mul(2).max(8).min(ceiling);
                    if self
                        .metrics
                        .try_reserve_exact(target - self.metrics.len())
                        .is_err()
                    {
                        collection = Err(failure(ErrorKind::OutOfMemory));
                        return;
                    }
                    if self.metrics.capacity() * size_of::<Entry>() > self.metric_budget {
                        collection = Err(failure(ErrorKind::BufferTooSmall));
                        return;
                    }
                }
                self.metrics.insert(
                    index,
                    Entry {
                        px: key.0,
                        ch,
                        metrics: None,
                        visible: false,
                    },
                );
            }
        });
        collection?;
        let mut start = 0;
        while start < self.metrics.len() {
            let px = self.metrics[start].px;
            let mut reader = open(k, px)?;
            let info = reader.info();
            while start < self.metrics.len() && self.metrics[start].px == px {
                let e = &mut self.metrics[start];
                e.metrics = Some(
                    reader
                        .find(e.ch)
                        .map_err(font_error)?
                        .map_or_else(|| missing_glyph_metrics(&info), |g| g.metrics),
                );
                start += 1;
            }
        }
        Ok(())
    }
    pub fn begin_visible(&mut self) {
        for e in &mut self.metrics {
            e.visible = false;
        }
    }
    pub fn mark_visible(
        &mut self,
        text: &[u8],
        initial: Style,
        latin: FontSet,
        body_px: u16,
        heading_px: u16,
    ) -> Result<()> {
        self.mark_visible_with_flags(text, style_flags(initial), latin, body_px, heading_px)
    }
    pub fn mark_visible_with_flags(
        &mut self,
        text: &[u8],
        initial_flags: u8,
        latin: FontSet,
        body_px: u16,
        heading_px: u16,
    ) -> Result<()> {
        let mut valid = true;
        scan(text, initial_flags, |ch, sty| {
            if needs(latin, ch, sty) {
                let key = (pixel_size(sty, body_px, heading_px), ch);
                if let Ok(i) = self.metrics.binary_search_by_key(&key, |e| (e.px, e.ch)) {
                    self.metrics[i].visible = true;
                } else {
                    valid = false;
                }
            }
        });
        if valid {
            Ok(())
        } else {
            Err(failure(ErrorKind::InvalidData))
        }
    }
    pub fn prepare_visible(
        &mut self,
        k: &mut KernelHandle<'_>,
        body_px: u16,
        heading_px: u16,
    ) -> Result<()> {
        // Unpublish both roles before any fallible work; errors cannot expose
        // either an old page or an otherwise successful partial preparation.
        let mut previous = [self.body.take(), self.heading.take()];
        let mut prepared = [None, None];
        for (role, px) in [(0, body_px), (1, heading_px)] {
            if role == 1 && (px == body_px || !self.heading_role) {
                continue;
            }
            let count = self
                .metrics
                .iter()
                .filter(|e| e.px == px && e.visible)
                .count();
            if count == 0 {
                continue;
            }
            let mut chars = buffer(count, '\0', SCRATCH_BUDGET)?;
            let mut bitmap_len = 0usize;
            for (i, e) in self
                .metrics
                .iter()
                .filter(|e| e.px == px && e.visible)
                .enumerate()
            {
                chars[i] = e.ch;
                let m = e.metrics.ok_or(failure(ErrorKind::InvalidData))?;
                bitmap_len = bitmap_len
                    .checked_add(bitmap_size(m.width, m.height) as usize)
                    .ok_or(failure(ErrorKind::BufferTooSmall))?;
            }
            let needed = size_of::<OwnedCache>() + count * size_of::<PageGlyphSlot>() + bitmap_len;
            let cache_budget = self.slot_budget + self.bitmap_budget + size_of::<OwnedCache>();
            if needed > cache_budget || bitmap_len > self.bitmap_budget {
                return Err(failure(ErrorKind::BufferTooSmall));
            }
            let (mut slots, mut bitmaps) = previous[role]
                .take()
                .map(|(_, cache)| cache.into_storage())
                .unwrap_or_else(|| (Box::default(), BigBuf::empty()));
            if slots.len() < count {
                // Release replaced storage first, avoiding an old+new peak.
                drop(slots);
                slots = buffer(count, PageGlyphSlot::default(), self.slot_budget)?;
            }
            if bitmaps.len() < bitmap_len {
                drop(bitmaps);
                bitmaps = BigBuf::zeroed(BufClass::FontGlyphs, bitmap_len)
                    .map_err(|_| failure(ErrorKind::OutOfMemory))?;
            }
            if bitmaps.len() > self.bitmap_budget {
                return Err(failure(ErrorKind::BufferTooSmall));
            }
            let mut cache = PageCache::new(slots, bitmaps, cache_budget)
                .map_err(|_| failure(ErrorKind::BufferTooSmall))?;
            let mut reader = open(k, px)?;
            cache.prepare(&mut reader, &chars).map_err(|e| match e {
                pulp_fontpack::PreparationError::Font(e) => font_error(e),
                _ => failure(ErrorKind::BufferTooSmall),
            })?;
            prepared[role] = Some((px, cache));
        }
        [self.body, self.heading] = prepared;
        Ok(())
    }
    pub fn prepare_text(
        &mut self,
        k: &mut KernelHandle<'_>,
        text: &[u8],
        latin: FontSet,
        body_px: u16,
        heading_px: u16,
    ) -> Result<()> {
        self.stage_metrics(k, text, latin, body_px, heading_px)?;
        self.mark_visible(text, Style::Regular, latin, body_px, heading_px)?;
        self.prepare_visible(k, body_px, heading_px)
    }
    pub fn view(&self, latin: FontSet, body_px: u16, heading_px: u16) -> LayoutFonts<'_> {
        LayoutFonts {
            latin,
            state: self,
            body_px,
            heading_px,
        }
    }
    pub fn get(&self, px: u16, ch: char) -> Option<Glyph<'_>> {
        self.body
            .iter()
            .chain(self.heading.iter())
            .find(|(size, _)| *size == px)?
            .1
            .get(ch)
    }
    pub fn has_fallback(&self) -> bool {
        !self.metrics.is_empty()
    }
    /// Whether the staged layout window depends on this fallback bank.
    pub fn uses_bank(&self, px: u16) -> bool {
        self.metrics.iter().any(|entry| entry.px == px)
    }
}
impl Default for CjkState {
    fn default() -> Self {
        Self::new()
    }
}
pub struct LayoutFonts<'a> {
    latin: FontSet,
    state: &'a CjkState,
    body_px: u16,
    heading_px: u16,
}
pub type PreparedFonts<'a> = LayoutFonts<'a>;
impl LayoutFonts<'_> {
    pub fn font(&self, sty: Style) -> &'static BitmapFont {
        self.latin.font(sty)
    }
    pub fn line_height(&self, sty: Style) -> u16 {
        if super::font_data::HAS_REGULAR {
            self.latin.line_height(sty)
        } else {
            18
        }
    }
    pub fn has_fallback(&self) -> bool {
        self.state.has_fallback()
    }
    pub fn advance(&self, ch: char, sty: Style) -> u16 {
        if !needs(self.latin, ch, sty) {
            return if super::font_data::HAS_REGULAR {
                self.latin.advance(ch, sty).into()
            } else {
                9
            };
        }
        let key = (pixel_size(sty, self.body_px, self.heading_px), ch);
        self.state
            .metrics
            .binary_search_by_key(&key, |e| (e.px, e.ch))
            .ok()
            .and_then(|i| self.state.metrics[i].metrics)
            .unwrap_or_else(|| missing_glyph_metrics(&fallback_info(key.0)))
            .advance
    }

    pub fn draw_char(
        &self,
        s: &mut StripBuffer,
        ch: char,
        sty: Style,
        x: i32,
        baseline: i32,
    ) -> u16 {
        self.draw_char_fg(s, ch, sty, BinaryColor::On, x, baseline)
    }
    pub fn draw_char_fg(
        &self,
        s: &mut StripBuffer,
        ch: char,
        sty: Style,
        fg: BinaryColor,
        x: i32,
        baseline: i32,
    ) -> u16 {
        if !needs(self.latin, ch, sty) {
            if !super::font_data::HAS_REGULAR {
                use embedded_graphics::{
                    mono_font::{MonoTextStyle, ascii::FONT_9X18},
                    prelude::*,
                    text::Text,
                };
                let mut bytes = [0; 4];
                let style = MonoTextStyle::new(&FONT_9X18, fg);
                let _ =
                    Text::new(ch.encode_utf8(&mut bytes), Point::new(x, baseline), style).draw(s);
                return 9;
            }
            return self
                .latin
                .font(sty)
                .draw_char_fg(s, ch, fg, x, baseline)
                .into();
        }
        let px = pixel_size(sty, self.body_px, self.heading_px);
        let Some(g) = self.state.get(px, ch) else {
            // An invariant failure remains drawable without reaching a source.
            // Preparation validates visible keys before the reader publishes Ready.
            use embedded_graphics::{
                prelude::*,
                primitives::{PrimitiveStyle, Rectangle},
            };
            let m = missing_glyph_metrics(&fallback_info(px));
            let _ = Rectangle::new(
                Point::new(x + i32::from(m.offset_x), baseline + i32::from(m.offset_y)),
                Size::new(m.width.into(), m.height.into()),
            )
            .into_styled(PrimitiveStyle::with_stroke(fg, 1))
            .draw(s);
            return m.advance;
        };
        let m = g.metrics;
        s.blit_1bpp(
            g.bitmap,
            0,
            m.width.into(),
            m.height.into(),
            usize::from(m.width).div_ceil(8),
            x + i32::from(m.offset_x),
            baseline + i32::from(m.offset_y),
            fg == BinaryColor::On,
        );
        m.advance
    }
}
fn fallback_info(pixel_size: u16) -> pulp_fontpack::FontInfo {
    pulp_fontpack::FontInfo {
        pixel_size,
        font_id: 0,
        line_height: pixel_size,
        ascent: pixel_size,
    }
}
fn pixel_size(sty: Style, body: u16, heading: u16) -> u16 {
    if sty == Style::Heading { heading } else { body }
}
fn needs(latin: FontSet, ch: char, sty: Style) -> bool {
    ch >= ' '
        && ch != '\u{fffd}'
        && ch != '\u{ad}'
        && ch != '\u{a0}'
        && !latin.font(sty).has_glyph(ch)
}
fn style_flags(style: Style) -> u8 {
    match style {
        Style::Regular => 0,
        Style::Bold => 1,
        Style::Italic => 2,
        Style::Heading => 4,
    }
}
fn scan(text: &[u8], initial_flags: u8, mut f: impl FnMut(char, Style)) {
    let mut styles = StyleState::from_flags(initial_flags);
    let mut i = 0;
    while i < text.len() {
        if text[i] == MARKER && i + 1 < text.len() {
            if text[i + 1] == IMG_REF
                && i + 2 < text.len()
                && i + 3 + text[i + 2] as usize <= text.len()
            {
                i += 3 + text[i + 2] as usize;
                continue;
            }
            styles.apply_marker(text[i + 1]);
            i += 2;
            continue;
        }
        let (ch, n) = pulp_kernel::util::decode_utf8_char(text, i);
        f(ch, styles.style());
        i += n;
    }
}

/// Temporary, bounded collection of unsupported visible label scalars.
/// Latin glyphs never cause font storage work.
pub struct VisibleText {
    bytes: Vec<u8>,
    error: Option<Error>,
}
impl VisibleText {
    pub const fn new() -> Self {
        Self {
            bytes: Vec::new(),
            error: None,
        }
    }
    pub fn add(&mut self, text: &str, font: &'static BitmapFont, heading: bool) {
        use smol_epub::html_strip::{HEADING_OFF, HEADING_ON};
        let mut marked = false;
        for ch in text.chars().filter(|&ch| ch >= ' ' && !font.has_glyph(ch)) {
            if !marked {
                self.append(&[MARKER, if heading { HEADING_ON } else { HEADING_OFF }]);
                marked = true;
            }
            let mut raw = [0; 4];
            self.append(ch.encode_utf8(&mut raw).as_bytes());
        }
    }
    fn append(&mut self, bytes: &[u8]) {
        if self.error.is_some() {
            return;
        }
        if self.bytes.len() + bytes.len() > 4096 {
            self.error = Some(failure(ErrorKind::BufferTooSmall));
            return;
        }
        if self.bytes.try_reserve_exact(bytes.len()).is_err() {
            self.error = Some(failure(ErrorKind::OutOfMemory));
            return;
        }
        if self.bytes.capacity() > 4096 {
            self.error = Some(failure(ErrorKind::BufferTooSmall));
            return;
        }
        self.bytes.extend_from_slice(bytes);
    }
    pub fn bytes(&self) -> Result<&[u8]> {
        if let Some(e) = self.error {
            Err(e)
        } else {
            Ok(&self.bytes)
        }
    }
}

/// One app's bounded label caches; reset on exit/suspend, never shared with
/// background work in another app. Preparation errors remain drawable.
pub struct SurfaceFonts {
    state: CjkState,
    size: u8,
    // (size, length, FNV-1a) of the last successful prepare. A collision can
    // only keep a stale glyph set drawable as missing-glyph boxes; draw never
    // reaches storage and indexes only prepared data.
    prepared: Option<(u8, usize, u32)>,
    pub error: Option<Error>,
}
impl SurfaceFonts {
    pub const fn new() -> Self {
        Self {
            state: CjkState::auxiliary(),
            size: 0,
            prepared: None,
            error: None,
        }
    }
    pub fn set_size(&mut self, idx: u8) {
        self.size = if idx < 5 { idx } else { 1 };
    }
    pub fn clear(&mut self) {
        self.state.clear();
        self.prepared = None;
        self.error = None;
    }
    pub fn prepare(&mut self, k: &mut KernelHandle<'_>, text: &VisibleText) {
        let idx = usize::from(self.size);
        let key = text
            .bytes()
            .ok()
            .map(|bytes| (self.size, bytes.len(), smol_epub::cache::fnv1a(bytes)));
        if key.is_some() && key == self.prepared && self.error.is_none() {
            return;
        }
        let result = text.bytes().and_then(|bytes| {
            self.state.prepare_text(
                k,
                bytes,
                FontSet::for_size(self.size),
                BODY_PIXELS[idx],
                HEADING_PIXELS[idx],
            )
        });
        self.error = result.err();
        if self.error.is_some() {
            self.state.clear();
        }
        self.prepared = key.filter(|_| self.error.is_none());
    }
    pub fn view(&self) -> PreparedFonts<'_> {
        let idx = usize::from(self.size);
        self.state.view(
            FontSet::for_size(self.size),
            BODY_PIXELS[idx],
            HEADING_PIXELS[idx],
        )
    }
}
