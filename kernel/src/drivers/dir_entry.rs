// directory entry type, the root-listing name filter, the TITLES.BIN line and
// the app-data directory names
//
// pure core code (no hardware, no embedded-sdmmc): this file is compiled into
// the firmware (drivers::dir_entry) and `#[path]`-included by the host crate
// (host/src/lib.rs), so firmware and host tests share one definition

use crate::util::utf8_prefix_len;

pub const PULP_DIR: &str = "_PULP";
pub const TITLES_FILE: &str = "TITLES.BIN";
pub const TITLE_CAP: usize = 64;

#[derive(Clone, Copy)]
pub struct DirEntry {
    pub name: [u8; 13],
    pub name_len: u8,
    pub is_dir: bool,
    pub size: u32,
    pub title: [u8; TITLE_CAP],
    pub title_len: u8,
}

impl DirEntry {
    pub const EMPTY: Self = Self {
        name: [0u8; 13],
        name_len: 0,
        is_dir: false,
        size: 0,
        title: [0u8; TITLE_CAP],
        title_len: 0,
    };

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len as usize]).unwrap_or("?")
    }

    pub fn display_name(&self) -> &str {
        let len = (self.title_len & 0x7F) as usize;
        if len > 0 {
            core::str::from_utf8(&self.title[..len]).unwrap_or(self.name_str())
        } else {
            self.name_str()
        }
    }

    pub fn has_real_title(&self) -> bool {
        self.title_len > 0 && self.title_len & 0x80 == 0
    }

    pub fn set_title(&mut self, s: &[u8]) {
        let n = utf8_prefix_len(s, TITLE_CAP);
        self.title[..n].copy_from_slice(&s[..n]);
        self.title_len = n as u8;
    }

    // write a humanized SFN into the title buffer as a soft fallback;
    // does not prevent the title scanner from resolving a real title
    pub fn humanize_sfn(&mut self) {
        let nlen = self.name_len as usize;
        if nlen == 0 || self.has_real_title() {
            return;
        }
        let src = &self.name[..nlen];
        // check if name is all-uppercase (typical 8.3 SFN)
        let all_upper = src.iter().all(|&b| !b.is_ascii_lowercase());
        if !all_upper {
            return; // mixed case: user-supplied LFN, leave as-is
        }
        let n = nlen.min(TITLE_CAP);
        let dot_pos = src.iter().position(|&b| b == b'.').unwrap_or(n);
        for i in 0..n {
            if i == 0 {
                self.title[i] = src[i]; // keep first char uppercase
            } else if i > dot_pos {
                self.title[i] = src[i].to_ascii_lowercase(); // lowercase ext
            } else {
                self.title[i] = src[i].to_ascii_lowercase();
            }
        }
        self.title_len = 0x80 | n as u8;
    }
}

fn ext_eq(name: &[u8], target: &[u8]) -> bool {
    let dot = match name.iter().rposition(|&b| b == b'.') {
        Some(p) => p,
        None => return false,
    };
    let ext = &name[dot + 1..];
    ext.len() == target.len() && ext.eq_ignore_ascii_case(target)
}

fn has_supported_ext(name: &[u8]) -> bool {
    ext_eq(name, b"TXT") || ext_eq(name, b"EPUB") || ext_eq(name, b"EPU") || ext_eq(name, b"MD")
}

// whether a root-directory entry name ("NAME.EXT" bytes) is listed by
// storage::list_root_files: not empty, not hidden ('.' / '_' prefix), and a
// supported extension
pub fn is_listed_name(sfn: &[u8]) -> bool {
    !sfn.is_empty() && sfn[0] != b'.' && sfn[0] != b'_' && has_supported_ext(sfn)
}

pub struct DirPage {
    pub total: usize,
    pub count: usize,
}

// one TITLES.BIN line, "name\ttitle\n" with the title cut to TITLE_CAP, written
// into `line`; returns its length, or None when it would not fit in 128 bytes
pub fn title_line(filename: &str, title: &str, line: &mut [u8; 128]) -> Option<usize> {
    let name_bytes = filename.as_bytes();
    let title_bytes = title.as_bytes();
    let title_len = utf8_prefix_len(title_bytes, TITLE_CAP);
    let line_len = name_bytes.len() + 1 + title_len + 1; // name + \t + title + \n
    if line_len > 128 {
        return None;
    }
    line[..name_bytes.len()].copy_from_slice(name_bytes);
    line[name_bytes.len()] = b'\t';
    line[name_bytes.len() + 1..name_bytes.len() + 1 + title_len]
        .copy_from_slice(&title_bytes[..title_len]);
    line[name_bytes.len() + 1 + title_len] = b'\n';
    Some(line_len)
}
