// file-content stores behind VirtualStorage: the only part that differs between
// the memory and the host-file backend. Paths are '/'-joined, relative to the
// card root ("" is the root directory), e.g. "_PULP/CACHE/P0.BIN".

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub(super) trait Fs {
    fn is_dir(&self, path: &str) -> bool;
    // create one directory level; the parent must exist
    fn mkdir(&mut self, path: &str) -> io::Result<()>;
    // byte length of a regular file, None when `path` is not a file
    fn len(&self, path: &str) -> Option<u32>;
    // read up to buf.len() bytes starting at `offset`, returns the count
    fn read_at(&self, path: &str, offset: u32, buf: &mut [u8]) -> io::Result<usize>;
    // create or truncate-and-replace
    fn write(&mut self, path: &str, data: &[u8]) -> io::Result<()>;
    // append, creating the file when missing
    fn append(&mut self, path: &str, data: &[u8]) -> io::Result<()>;
    // overwrite from `offset` (<= current length) in an existing file
    fn write_at(&mut self, path: &str, offset: u32, data: &[u8]) -> io::Result<()>;
    fn remove(&mut self, path: &str) -> io::Result<()>;
    // remove an empty directory
    fn remove_dir(&mut self, path: &str) -> io::Result<()>;
    // direct children of a directory: (name, is_dir, size)
    fn list(&self, dir: &str) -> io::Result<Vec<(String, bool, u32)>>;
}

fn not_found() -> io::Error {
    io::ErrorKind::NotFound.into()
}

// ---------------------------------------------------------------------------

#[derive(Default)]
pub(super) struct MemFs {
    files: HashMap<String, Vec<u8>>,
    dirs: HashSet<String>,
}

impl MemFs {
    // directories are implied by the path prefixes of the seeded files
    pub(super) fn with_files(files: &[(&str, &[u8])]) -> Self {
        let mut fs = Self::default();
        for (path, data) in files {
            for (i, _) in path.match_indices('/') {
                fs.dirs.insert(path[..i].to_string());
            }
            fs.files.insert((*path).to_string(), data.to_vec());
        }
        fs
    }

    fn parent_is_dir(&self, path: &str) -> bool {
        match path.rsplit_once('/') {
            Some((parent, _)) => self.is_dir(parent),
            None => true,
        }
    }
}

impl Fs for MemFs {
    fn is_dir(&self, path: &str) -> bool {
        path.is_empty() || self.dirs.contains(path)
    }

    fn mkdir(&mut self, path: &str) -> io::Result<()> {
        if !self.parent_is_dir(path) {
            return Err(not_found());
        }
        if self.files.contains_key(path) || self.dirs.contains(path) {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        self.dirs.insert(path.to_string());
        Ok(())
    }

    fn len(&self, path: &str) -> Option<u32> {
        self.files.get(path).map(|f| f.len() as u32)
    }

    fn read_at(&self, path: &str, offset: u32, buf: &mut [u8]) -> io::Result<usize> {
        let file = self.files.get(path).ok_or_else(not_found)?;
        let start = (offset as usize).min(file.len());
        let n = buf.len().min(file.len() - start);
        buf[..n].copy_from_slice(&file[start..start + n]);
        Ok(n)
    }

    fn write(&mut self, path: &str, data: &[u8]) -> io::Result<()> {
        if !self.parent_is_dir(path) || self.dirs.contains(path) {
            return Err(not_found());
        }
        self.files.insert(path.to_string(), data.to_vec());
        Ok(())
    }

    fn append(&mut self, path: &str, data: &[u8]) -> io::Result<()> {
        if !self.parent_is_dir(path) || self.dirs.contains(path) {
            return Err(not_found());
        }
        self.files
            .entry(path.to_string())
            .or_default()
            .extend_from_slice(data);
        Ok(())
    }

    fn write_at(&mut self, path: &str, offset: u32, data: &[u8]) -> io::Result<()> {
        let file = self.files.get_mut(path).ok_or_else(not_found)?;
        let start = offset as usize;
        if start > file.len() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        if file.len() < start + data.len() {
            file.resize(start + data.len(), 0);
        }
        file[start..start + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn remove(&mut self, path: &str) -> io::Result<()> {
        self.files.remove(path).map(|_| ()).ok_or_else(not_found)
    }

    fn remove_dir(&mut self, path: &str) -> io::Result<()> {
        let prefix = format!("{path}/");
        let has_children = self.files.keys().any(|p| p.starts_with(&prefix))
            || self.dirs.iter().any(|d| d.starts_with(&prefix));
        if has_children {
            return Err(io::ErrorKind::DirectoryNotEmpty.into());
        }
        if self.dirs.remove(path) {
            Ok(())
        } else {
            Err(not_found())
        }
    }

    fn list(&self, dir: &str) -> io::Result<Vec<(String, bool, u32)>> {
        if !self.is_dir(dir) {
            return Err(not_found());
        }
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let child = |p: &str| -> Option<String> {
            let rest = p.strip_prefix(prefix.as_str())?;
            (!rest.is_empty() && !rest.contains('/')).then(|| rest.to_string())
        };
        let mut out = Vec::new();
        for (p, data) in &self.files {
            if let Some(name) = child(p) {
                out.push((name, false, data.len() as u32));
            }
        }
        for p in &self.dirs {
            if let Some(name) = child(p) {
                out.push((name, true, 0));
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------

pub(super) struct HostFs {
    root: PathBuf,
}

impl HostFs {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    fn full(&self, path: &str) -> PathBuf {
        path.split('/')
            .filter(|c| !c.is_empty())
            .fold(self.root.clone(), |p, c| p.join(c))
    }
}

impl Fs for HostFs {
    fn is_dir(&self, path: &str) -> bool {
        self.full(path).is_dir()
    }

    fn mkdir(&mut self, path: &str) -> io::Result<()> {
        std::fs::create_dir(self.full(path))
    }

    fn len(&self, path: &str) -> Option<u32> {
        let meta = std::fs::metadata(self.full(path)).ok()?;
        meta.is_file().then(|| meta.len() as u32)
    }

    fn read_at(&self, path: &str, offset: u32, buf: &mut [u8]) -> io::Result<usize> {
        let mut file = std::fs::File::open(self.full(path))?;
        file.seek(SeekFrom::Start(u64::from(offset)))?;
        let mut n = 0;
        while n < buf.len() {
            match file.read(&mut buf[n..])? {
                0 => break,
                k => n += k,
            }
        }
        Ok(n)
    }

    fn write(&mut self, path: &str, data: &[u8]) -> io::Result<()> {
        std::fs::write(self.full(path), data)
    }

    fn append(&mut self, path: &str, data: &[u8]) -> io::Result<()> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.full(path))?
            .write_all(data)
    }

    fn write_at(&mut self, path: &str, offset: u32, data: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).open(self.full(path))?;
        file.seek(SeekFrom::Start(u64::from(offset)))?;
        file.write_all(data)
    }

    fn remove(&mut self, path: &str) -> io::Result<()> {
        std::fs::remove_file(self.full(path))
    }

    fn remove_dir(&mut self, path: &str) -> io::Result<()> {
        std::fs::remove_dir(self.full(path))
    }

    fn list(&self, dir: &str) -> io::Result<Vec<(String, bool, u32)>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(self.full(dir))? {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                continue; // not representable as a card file name
            };
            let meta = entry.metadata()?;
            out.push((name, meta.is_dir(), meta.len() as u32));
        }
        Ok(out)
    }
}
