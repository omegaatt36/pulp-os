// Pulp font pack (.PFP) v1 loader and random-access glyph lookup
//
// contract: docs/font-pack.txt. the pack stays on the SD card; RAM holds
// the parsed header, the cached fallback record and first_cp, the code
// point of every 32nd index record (one per 512-byte index sector,
// at most 2,048 entries = 8 KiB). no alloc: first_cp is a fixed array
//
// load runs the §6 checks in order and returns the first failure; a
// lookup then costs one index-sector read plus one bitmap read, and
// needs no further validation because load proved every record in bounds

use crate::crc32::{Crc32, crc32};

const MAGIC: &[u8; 8] = b"PULPFONT";
const HEADER_LEN: usize = 64;
const INDEX_OFF: u32 = 512;
const SECTOR: usize = 512;
const RECORD_LEN: usize = 16;
const RECORDS_PER_SECTOR: u32 = (SECTOR / RECORD_LEN) as u32;
const MAX_GLYPHS: u32 = 65_536;
const MAX_SECTORS: usize = (MAX_GLYPHS / RECORDS_PER_SECTOR) as usize;

// failure reported by the storage layer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoError {
    // the pack file (or its directory) does not exist
    NotFound,
    // any other open / seek / read failure
    Io,
}

// §6 load errors, in check order; the first failing check is returned
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadError {
    NotFound,
    Io,
    // actual size < 64, or < file_len
    Truncated,
    BadMagic,
    UnsupportedVersion,
    HeaderChecksumMismatch,
    BadHeader,
    WrongPixelSize,
    BadLayout,
    TrailingData,
    IndexChecksumMismatch,
    InvalidCodePoint,
    Unsorted,
    BitmapOutOfBounds,
    FallbackMissing,
}

// errors possible after a successful load (§6 runtime lookup): the card
// failed or was swapped, or the caller's bitmap buffer is too small
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupError {
    Io,
    // a read came back short
    Truncated,
    // buffer shorter than the glyph's bitmap; size it from max_glyph_len
    BufferTooSmall,
}

impl From<IoError> for LoadError {
    fn from(e: IoError) -> Self {
        match e {
            IoError::NotFound => LoadError::NotFound,
            IoError::Io => LoadError::Io,
        }
    }
}

// random-access view of one pack file; the firmware implements it over
// its SD/FAT storage, the host over a byte slice
pub trait PackReader {
    // actual file size in bytes
    fn size(&mut self) -> Result<u32, IoError>;
    // read up to buf.len() bytes at offset; returns the count read, which
    // is short only at end of file
    fn read_at(&mut self, offset: u32, buf: &mut [u8]) -> Result<usize, IoError>;
}

// a pack held in memory (host converter self-check, tests)
pub struct SliceReader<'a>(pub &'a [u8]);

impl PackReader for SliceReader<'_> {
    fn size(&mut self) -> Result<u32, IoError> {
        u32::try_from(self.0.len()).map_err(|_| IoError::Io)
    }

    fn read_at(&mut self, offset: u32, buf: &mut [u8]) -> Result<usize, IoError> {
        let start = (offset as usize).min(self.0.len());
        let n = buf.len().min(self.0.len() - start);
        buf[..n].copy_from_slice(&self.0[start..start + n]);
        Ok(n)
    }
}

// one §4 index record: BitmapGlyph metrics with a u32 section-relative
// bitmap offset
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackGlyph {
    pub code_point: u32,
    pub bitmap_offset: u32,
    pub width: u8,
    pub height: u8,
    pub advance: u8,
    pub offset_x: i8,
    pub offset_y: i8,
}

impl PackGlyph {
    fn parse(r: &[u8]) -> Self {
        PackGlyph {
            code_point: le32(r, 0),
            bitmap_offset: le32(r, 4),
            width: r[8],
            height: r[9],
            advance: r[10],
            offset_x: r[11] as i8,
            offset_y: r[12] as i8,
        }
    }

    // §5: ceil(width / 8) bytes per row, height rows (<= 8,160)
    pub fn bitmap_len(&self) -> usize {
        (self.width as usize).div_ceil(8) * self.height as usize
    }
}

// what resolve chose for a char: the record whose metrics lay it out
// and whose bitmap draws it; fallback is true when the char is absent
// and glyph is the fallback_cp record
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    pub glyph: PackGlyph,
    pub fallback: bool,
}

pub struct FontPack {
    first_cp: [u32; MAX_SECTORS],
    glyph_count: u32,
    bitmap_off: u32,
    fallback: PackGlyph,
    pixel_size: u16,
    line_height: u16,
    ascent: u16,
    max_glyph_len: u16,
}

impl FontPack {
    // validate the pack per §6 and build the lookup table in one
    // streaming pass over the index; pixel_size is the size requested
    // (§7 file name), checked as #8. only reads through the reader
    pub fn load<R: PackReader>(reader: &mut R, pixel_size: u16) -> Result<Self, LoadError> {
        let size = reader.size()?;
        if size < HEADER_LEN as u32 {
            return Err(LoadError::Truncated);
        }
        let mut h = [0u8; HEADER_LEN];
        read_exact(reader, 0, &mut h).map_err(|e| e.into_load())?;

        if &h[0..8] != MAGIC {
            return Err(LoadError::BadMagic);
        }
        if le16(&h, 8) != 1 {
            return Err(LoadError::UnsupportedVersion);
        }
        if crc32(&h[0..60]) != le32(&h, 60) {
            return Err(LoadError::HeaderChecksumMismatch);
        }

        let header_len = le16(&h, 10);
        let file_pixel_size = le16(&h, 12);
        let line_height = le16(&h, 14);
        let ascent = le16(&h, 16);
        let glyph_count = le32(&h, 20);
        let fallback_cp = le32(&h, 24);
        let index_off = le32(&h, 28);
        let index_len = le32(&h, 32);
        let bitmap_off = le32(&h, 36);
        let bitmap_len = le32(&h, 40);
        let license_off = le32(&h, 44);
        let license_len = le32(&h, 48);
        let file_len = le32(&h, 52);
        let index_crc = le32(&h, 56);

        if header_len as usize != HEADER_LEN
            || !(8..=96).contains(&file_pixel_size)
            || line_height == 0
            || ascent > line_height
            || !(1..=MAX_GLYPHS).contains(&glyph_count)
            || license_len == 0
        {
            return Err(LoadError::BadHeader);
        }
        if file_pixel_size != pixel_size {
            return Err(LoadError::WrongPixelSize);
        }

        let layout_ok = index_off == INDEX_OFF
            && glyph_count.checked_mul(RECORD_LEN as u32) == Some(index_len)
            && index_off.checked_add(index_len) == Some(bitmap_off)
            && bitmap_off.checked_add(bitmap_len) == Some(license_off)
            && license_off.checked_add(license_len) == Some(file_len);
        if !layout_ok {
            return Err(LoadError::BadLayout);
        }
        if size < file_len {
            return Err(LoadError::Truncated);
        }
        if size > file_len {
            return Err(LoadError::TrailingData);
        }

        // #12..#16: one pass; the CRC outranks the first record failure
        let mut first_cp = [0u32; MAX_SECTORS];
        let mut crc = Crc32::new();
        let mut record_err = None;
        let mut prev_cp = None;
        let mut fallback = None;
        let mut max_len = 0usize;
        let mut buf = [0u8; SECTOR];
        let mut done = 0u32;
        let mut sector = 0usize;
        while done < index_len {
            let n = (index_len - done).min(SECTOR as u32) as usize;
            let chunk = &mut buf[..n];
            read_exact(reader, index_off + done, chunk).map_err(|e| e.into_load())?;
            crc.update(chunk);
            for rec in chunk.as_chunks::<RECORD_LEN>().0 {
                let g = PackGlyph::parse(rec);
                if record_err.is_none() {
                    record_err = check_record(&g, prev_cp, bitmap_len);
                }
                prev_cp = Some(g.code_point);
                max_len = max_len.max(g.bitmap_len());
                if g.code_point == fallback_cp && fallback.is_none() {
                    fallback = Some(g);
                }
            }
            first_cp[sector] = le32(chunk, 0);
            sector += 1;
            done += n as u32;
        }
        if crc.finish() != index_crc {
            return Err(LoadError::IndexChecksumMismatch);
        }
        if let Some(e) = record_err {
            return Err(e);
        }
        let fallback = fallback.ok_or(LoadError::FallbackMissing)?;

        Ok(FontPack {
            first_cp,
            glyph_count,
            bitmap_off,
            fallback,
            pixel_size: file_pixel_size,
            line_height,
            ascent,
            // bitmap_len() <= 32 * 255
            max_glyph_len: max_len as u16,
        })
    }

    pub fn pixel_size(&self) -> u16 {
        self.pixel_size
    }

    pub fn line_height(&self) -> u16 {
        self.line_height
    }

    pub fn ascent(&self) -> u16 {
        self.ascent
    }

    pub fn glyph_count(&self) -> u32 {
        self.glyph_count
    }

    pub fn bitmap_off(&self) -> u32 {
        self.bitmap_off
    }

    pub fn fallback_cp(&self) -> u32 {
        self.fallback.code_point
    }

    // record for fallback_cp, cached at load (§6)
    pub fn fallback_glyph(&self) -> PackGlyph {
        self.fallback
    }

    // largest glyph bitmap in the pack; a buffer this long fits any glyph
    pub fn max_glyph_len(&self) -> usize {
        self.max_glyph_len as usize
    }

    // record for cp, or None when the pack does not contain it;
    // reads at most one index sector
    pub fn find<R: PackReader>(
        &self,
        reader: &mut R,
        cp: u32,
    ) -> Result<Option<PackGlyph>, LookupError> {
        let sectors = self.glyph_count.div_ceil(RECORDS_PER_SECTOR) as usize;
        let k = self.first_cp[..sectors].partition_point(|&first| first <= cp);
        if k == 0 {
            return Ok(None);
        }
        let sector = (k - 1) as u32;
        let records = (self.glyph_count - sector * RECORDS_PER_SECTOR).min(RECORDS_PER_SECTOR);
        let mut buf = [0u8; SECTOR];
        let chunk = &mut buf[..records as usize * RECORD_LEN];
        read_exact(reader, INDEX_OFF + sector * SECTOR as u32, chunk)
            .map_err(|e| e.into_lookup())?;

        let (mut lo, mut hi) = (0usize, records as usize);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let rec = &chunk[mid * RECORD_LEN..(mid + 1) * RECORD_LEN];
            let mid_cp = le32(rec, 0);
            if mid_cp == cp {
                return Ok(Some(PackGlyph::parse(rec)));
            }
            if mid_cp < cp {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Ok(None)
    }

    // read g's bitmap into the front of buf and return that slice
    pub fn read_bitmap<'b, R: PackReader>(
        &self,
        reader: &mut R,
        g: &PackGlyph,
        buf: &'b mut [u8],
    ) -> Result<&'b [u8], LookupError> {
        let len = g.bitmap_len();
        let out = buf.get_mut(..len).ok_or(LookupError::BufferTooSmall)?;
        if len > 0 {
            let offset = self
                .bitmap_off
                .checked_add(g.bitmap_offset)
                .ok_or(LookupError::Truncated)?;
            read_exact(reader, offset, out).map_err(|e| e.into_lookup())?;
        }
        Ok(out)
    }

    // R4: the one resolver behind measurement and drawing. a code point
    // in the pack resolves to its own record, whatever its size (a
    // zero-width space is present, not missing); only an absent one
    // resolves to the fallback record cached at load. glyph.code_point
    // names the bitmap to draw. reads at most one index sector, and no
    // bitmap. absent control characters (e.g. '\n') also resolve to the
    // fallback, so layout must consume them before resolving
    pub fn resolve<R: PackReader>(
        &self,
        reader: &mut R,
        ch: char,
    ) -> Result<Resolved, LookupError> {
        Ok(match self.find(reader, ch as u32)? {
            Some(glyph) => Resolved {
                glyph,
                fallback: false,
            },
            None => Resolved {
                glyph: self.fallback,
                fallback: true,
            },
        })
    }

    // pen advance of ch in px, as drawn (via resolve)
    pub fn advance<R: PackReader>(&self, reader: &mut R, ch: char) -> Result<u8, LookupError> {
        Ok(self.resolve(reader, ch)?.glyph.advance)
    }

    // the glyph to draw for ch (via resolve) and its bitmap, read into
    // the front of buf
    pub fn lookup<'b, R: PackReader>(
        &self,
        reader: &mut R,
        ch: char,
        buf: &'b mut [u8],
    ) -> Result<(Resolved, &'b [u8]), LookupError> {
        let res = self.resolve(reader, ch)?;
        Ok((res, self.read_bitmap(reader, &res.glyph, buf)?))
    }
}

// which font text renders with: any load error keeps the built-in
// BitmapFont (R5). the Pack variant carries the 8 KiB table inline
// (no alloc here); the firmware keeps this in a static or a Box
#[allow(clippy::large_enum_variant)]
pub enum ActiveFont {
    BuiltIn,
    Pack(FontPack),
}

impl ActiveFont {
    pub fn select(loaded: Result<FontPack, LoadError>) -> Self {
        match loaded {
            Ok(pack) => ActiveFont::Pack(pack),
            Err(_) => ActiveFont::BuiltIn,
        }
    }
}

// §6 per-record checks #13 -> #14 -> #15
fn check_record(g: &PackGlyph, prev_cp: Option<u32>, bitmap_len: u32) -> Option<LoadError> {
    if char::from_u32(g.code_point).is_none() {
        return Some(LoadError::InvalidCodePoint);
    }
    if prev_cp.is_some_and(|prev| g.code_point <= prev) {
        return Some(LoadError::Unsorted);
    }
    if g.bitmap_offset as u64 + g.bitmap_len() as u64 > bitmap_len as u64 {
        return Some(LoadError::BitmapOutOfBounds);
    }
    None
}

enum ReadFail {
    Io(IoError),
    Short,
}

impl ReadFail {
    fn into_load(self) -> LoadError {
        match self {
            ReadFail::Io(e) => e.into(),
            ReadFail::Short => LoadError::Truncated,
        }
    }

    fn into_lookup(self) -> LookupError {
        match self {
            ReadFail::Io(_) => LookupError::Io,
            ReadFail::Short => LookupError::Truncated,
        }
    }
}

// fill buf from offset, tolerating partial reads; a zero-byte read
// before buf is full means the file is shorter than expected
fn read_exact<R: PackReader>(reader: &mut R, offset: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
    let mut filled = 0;
    while filled < buf.len() {
        let at = offset.checked_add(filled as u32).ok_or(ReadFail::Short)?;
        let n = reader
            .read_at(at, &mut buf[filled..])
            .map_err(ReadFail::Io)?;
        if n == 0 {
            return Err(ReadFail::Short);
        }
        filled += n.min(buf.len() - filled);
    }
    Ok(())
}

fn le16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn le32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
