//! Pure Rust CJK font bundle builder with artifact verification and caching.
//! Replaces scripts/build-cjk-fonts.py and eliminates Python dependency.

use crate::{ConversionOptions, Input, convert_with_options};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const STAMP: &str = "BUNDLE.JSON";
pub const FORMAT: &str = "pulp-cjk-bundle-v1";

pub struct TempDir {
    pub path: PathBuf,
}

// The clock alone is not unique: macOS reports microseconds, so threads of one
// process asking in the same microsecond would share a directory.
static TEMP_DIR_SEQ: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    pub fn new_in(parent: &Path, prefix: &str) -> std::io::Result<Self> {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pid = std::process::id();
        let seq = TEMP_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!("{prefix}{pid}_{ts}_{seq}"));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(i64),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

impl JsonValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            JsonValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&BTreeMap<String, JsonValue>> {
        match self {
            JsonValue::Object(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        match self {
            JsonValue::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.as_object().and_then(|m| m.get(key))
    }

    pub fn to_canonical_string(&self) -> String {
        match self {
            JsonValue::Null => "null".to_string(),
            JsonValue::Bool(b) => {
                if *b {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            JsonValue::Number(n) => n.to_string(),
            JsonValue::String(s) => {
                let mut out = String::from("\"");
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        _ => out.push(c),
                    }
                }
                out.push('"');
                out
            }
            JsonValue::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|v| v.to_canonical_string()).collect();
                format!("[{}]", items.join(","))
            }
            JsonValue::Object(map) => {
                let mut items = Vec::new();
                for (k, v) in map {
                    items.push(format!(
                        "{}:{}",
                        JsonValue::String(k.clone()).to_canonical_string(),
                        v.to_canonical_string()
                    ));
                }
                format!("{{{}}}", items.join(","))
            }
        }
    }

    pub fn to_pretty_string(&self, indent: usize) -> String {
        match self {
            JsonValue::Object(map) => {
                if map.is_empty() {
                    return "{}".to_string();
                }
                let pad = " ".repeat(indent + 2);
                let mut lines = Vec::new();
                for (k, v) in map {
                    lines.push(format!(
                        "{}\"{}\": {}",
                        pad,
                        k,
                        v.to_pretty_string(indent + 2)
                    ));
                }
                format!("{{\n{}\n{}}}", lines.join(",\n"), " ".repeat(indent))
            }
            JsonValue::Array(arr) => {
                if arr.is_empty() {
                    return "[]".to_string();
                }
                let pad = " ".repeat(indent + 2);
                let lines: Vec<String> = arr
                    .iter()
                    .map(|v| format!("{}{}", pad, v.to_pretty_string(indent + 2)))
                    .collect();
                format!("[\n{}\n{}]", lines.join(",\n"), " ".repeat(indent))
            }
            _ => self.to_canonical_string(),
        }
    }
}

pub struct JsonParser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> JsonParser<'a> {
    pub fn parse_str(s: &'a str) -> Result<JsonValue, String> {
        let mut p = JsonParser {
            chars: s.chars().peekable(),
        };
        p.skip_whitespace();
        let val = p.parse_value()?;
        p.skip_whitespace();
        if p.chars.next().is_some() {
            return Err("trailing characters in JSON".to_string());
        }
        Ok(val)
    }

    fn skip_whitespace(&mut self) {
        while let Some(&c) = self.chars.peek() {
            if c.is_whitespace() {
                self.chars.next();
            } else {
                break;
            }
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_whitespace();
        match self.chars.peek() {
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('"') => self.parse_string().map(JsonValue::String),
            Some('t') | Some('f') => self.parse_bool(),
            Some('n') => self.parse_null(),
            Some(&c) if c.is_ascii_digit() || c == '-' => self.parse_number(),
            Some(c) => Err(format!("unexpected character '{c}'")),
            None => Err("unexpected EOF".to_string()),
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.chars.next(); // eat '"'
        let mut s = String::new();
        while let Some(c) = self.chars.next() {
            match c {
                '"' => return Ok(s),
                '\\' => match self.chars.next() {
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some('/') => s.push('/'),
                    Some('n') => s.push('\n'),
                    Some('r') => s.push('\r'),
                    Some('t') => s.push('\t'),
                    Some('u') => {
                        let mut hex = String::new();
                        for _ in 0..4 {
                            hex.push(self.chars.next().ok_or("incomplete \\u escape")?);
                        }
                        let code = u32::from_str_radix(&hex, 16).map_err(|e| e.to_string())?;
                        let ch = char::from_u32(code).ok_or("invalid unicode escape")?;
                        s.push(ch);
                    }
                    Some(other) => s.push(other),
                    None => return Err("unterminated string escape".to_string()),
                },
                other => s.push(other),
            }
        }
        Err("unterminated string".to_string())
    }

    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let mut num_str = String::new();
        while let Some(&c) = self.chars.peek() {
            if c.is_ascii_digit() || c == '-' {
                num_str.push(c);
                self.chars.next();
            } else {
                break;
            }
        }
        let n = num_str.parse::<i64>().map_err(|e| e.to_string())?;
        Ok(JsonValue::Number(n))
    }

    fn parse_bool(&mut self) -> Result<JsonValue, String> {
        if self.chars.peek() == Some(&'t') {
            for expected in "true".chars() {
                if self.chars.next() != Some(expected) {
                    return Err("invalid bool true".to_string());
                }
            }
            Ok(JsonValue::Bool(true))
        } else {
            for expected in "false".chars() {
                if self.chars.next() != Some(expected) {
                    return Err("invalid bool false".to_string());
                }
            }
            Ok(JsonValue::Bool(false))
        }
    }

    fn parse_null(&mut self) -> Result<JsonValue, String> {
        for expected in "null".chars() {
            if self.chars.next() != Some(expected) {
                return Err("invalid null".to_string());
            }
        }
        Ok(JsonValue::Null)
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.chars.next(); // eat '['
        let mut arr = Vec::new();
        self.skip_whitespace();
        if let Some(&']') = self.chars.peek() {
            self.chars.next();
            return Ok(JsonValue::Array(arr));
        }
        loop {
            arr.push(self.parse_value()?);
            self.skip_whitespace();
            match self.chars.next() {
                Some(',') => self.skip_whitespace(),
                Some(']') => break,
                _ => return Err("expected ',' or ']' in array".to_string()),
            }
        }
        Ok(JsonValue::Array(arr))
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.chars.next(); // eat '{'
        let mut map = BTreeMap::new();
        self.skip_whitespace();
        if let Some(&'}') = self.chars.peek() {
            self.chars.next();
            return Ok(JsonValue::Object(map));
        }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();
            if self.chars.next() != Some(':') {
                return Err("expected ':' after object key".to_string());
            }
            let val = self.parse_value()?;
            map.insert(key, val);
            self.skip_whitespace();
            match self.chars.next() {
                Some(',') => self.skip_whitespace(),
                Some('}') => break,
                _ => return Err("expected ',' or '}' in object".to_string()),
            }
        }
        Ok(JsonValue::Object(map))
    }
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

pub fn sha256_file(path: &Path) -> Result<String, std::io::Error> {
    let bytes = fs::read(path)?;
    Ok(sha256_bytes(&bytes))
}

pub struct ManifestInputs {
    pub data: JsonValue,
    pub font_path: PathBuf,
    pub license_path: PathBuf,
    pub require_chars_path: Option<PathBuf>,
}

pub fn load_manifest(path: &Path) -> Result<ManifestInputs, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("cannot read manifest: {e}"))?;
    let data = JsonParser::parse_str(&content)?;

    let schema = data
        .get("schema_version")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if schema != 1 {
        return Err("unsupported manifest schema_version".to_string());
    }

    for key in ["name", "version", "upstream_url", "license_name"] {
        let val = data.get(key).and_then(|v| v.as_str());
        match val {
            Some(s)
                if !s.is_empty() && s.trim() == s && s.bytes().all(|b| (32..=126).contains(&b)) => {
            }
            _ => return Err(format!("{key} must be non-empty printable ASCII")),
        }
    }

    let sizes = data
        .get("sizes")
        .and_then(|v| v.as_array())
        .ok_or("sizes must be array")?;
    if sizes.is_empty() {
        return Err("sizes must be distinct integers in 1..255".to_string());
    }
    let mut seen_sizes = std::collections::BTreeSet::new();
    for s in sizes {
        let n = s
            .as_i64()
            .ok_or("sizes must be distinct integers in 1..255")?;
        if !(1..=255).contains(&n) || !seen_sizes.insert(n) {
            return Err("sizes must be distinct integers in 1..255".to_string());
        }
    }

    let manifest_dir = path.parent().unwrap_or(Path::new("."));

    let load_input = |key: &str| -> Result<Option<PathBuf>, String> {
        let item = match data.get(key) {
            Some(i) => i,
            None => return Ok(None),
        };
        let p = item
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or(format!("{key} requires path and sha256"))?;
        let digest = item
            .get("sha256")
            .and_then(|v| v.as_str())
            .ok_or(format!("{key} requires path and sha256"))?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(format!("{key} requires lowercase SHA256"));
        }
        let src = manifest_dir
            .join(p)
            .canonicalize()
            .map_err(|e| format!("{key} path error: {e}"))?;
        let real_digest =
            sha256_file(&src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
        if real_digest != digest {
            return Err(format!("{key} SHA256 mismatch: {}", src.display()));
        }
        Ok(Some(src))
    };

    let font_path = load_input("font")?.ok_or("missing font in manifest")?;
    let license_path = load_input("license")?.ok_or("missing license in manifest")?;
    let require_chars_path = load_input("require_chars")?;

    Ok(ManifestInputs {
        data,
        font_path,
        license_path,
        require_chars_path,
    })
}

fn is_owned_dir(dir: &Path) -> bool {
    if !dir.is_dir() || dir.is_symlink() {
        return false;
    }
    let stamp = dir.join(STAMP);
    if !stamp.is_file() || stamp.is_symlink() {
        return false;
    }
    if let Ok(content) = fs::read_to_string(&stamp) {
        if let Ok(json) = JsonParser::parse_str(&content) {
            return json.get("format").and_then(|v| v.as_str()) == Some(FORMAT);
        }
    }
    false
}

fn is_verified_artifact(dir: &Path, key: &str) -> bool {
    if !is_owned_dir(dir) {
        return false;
    }
    let stamp = dir.join(STAMP);
    let content = match fs::read_to_string(&stamp) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let json = match JsonParser::parse_str(&content) {
        Ok(j) => j,
        Err(_) => return false,
    };
    if json.get("cache_key").and_then(|v| v.as_str()) != Some(key) {
        return false;
    }
    let files = match json.get("files").and_then(|v| v.as_object()) {
        Some(m) if !m.is_empty() => m,
        _ => return false,
    };

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let mut found_names = std::collections::BTreeSet::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        found_names.insert(name);
    }

    let mut expected_names = std::collections::BTreeSet::new();
    expected_names.insert(STAMP.to_string());
    for (name, digest_val) in files {
        expected_names.insert(name.clone());
        let expected_digest = match digest_val.as_str() {
            Some(d) => d,
            None => return false,
        };
        let file_path = dir.join(name);
        if file_path.is_symlink() || !file_path.is_file() {
            return false;
        }
        let actual_digest = match sha256_file(&file_path) {
            Ok(d) => d,
            Err(_) => return false,
        };
        if actual_digest != expected_digest {
            return false;
        }
    }

    found_names == expected_names
}

pub fn build_bundle(
    manifest_path: &Path,
    out_dir: &Path,
    cache_dir: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    let manifest_path = manifest_path.canonicalize()?;
    let inputs = load_manifest(&manifest_path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

    if out_dir == cache_dir || out_dir.starts_with(cache_dir) || cache_dir.starts_with(out_dir) {
        return Err("output and cache directories must be separate".into());
    }

    let destination = out_dir.join("_PULP/FONTS");
    let dest_parent = destination.parent().unwrap();
    if dest_parent.is_symlink() {
        return Err(format!("refusing symlink SD parent: {}", dest_parent.display()).into());
    }
    if (destination.exists() || destination.is_symlink()) && !is_owned_dir(&destination) {
        return Err(format!(
            "refusing to replace unowned font directory: {}",
            destination.display()
        )
        .into());
    }

    // Build fingerprint
    let rustc_ver = std::process::Command::new("rustc").arg("-vV").output()?;
    let rustc_str = String::from_utf8_lossy(&rustc_ver.stdout).to_string();

    let mut identity_map = BTreeMap::new();
    identity_map.insert("manifest".to_string(), inputs.data.clone());
    let mut build_map = BTreeMap::new();
    build_map.insert("rustc".to_string(), JsonValue::String(rustc_str));
    identity_map.insert("build".to_string(), JsonValue::Object(build_map));

    let identity_val = JsonValue::Object(identity_map.clone());
    let key = sha256_bytes(identity_val.to_canonical_string().as_bytes());

    fs::create_dir_all(cache_dir)?;
    let artifact = cache_dir.join(&key);
    if artifact.is_symlink() {
        return Err(format!("refusing symlink cache artifact: {}", artifact.display()).into());
    }

    let hit = is_verified_artifact(&artifact, &key);
    if !hit {
        let temp_dir = TempDir::new_in(cache_dir, "font-build-")?;
        let packs = temp_dir.path().join("packs");
        fs::create_dir_all(&packs)?;

        let font_bytes = fs::read(&inputs.font_path)?;
        let license_bytes = fs::read(&inputs.license_path)?;
        let require_text = match &inputs.require_chars_path {
            Some(p) => Some(fs::read_to_string(p)?),
            None => None,
        };

        let sizes_raw = inputs.data.get("sizes").unwrap().as_array().unwrap();
        let sizes: Vec<u16> = sizes_raw
            .iter()
            .map(|v| v.as_i64().unwrap() as u16)
            .collect();
        let license_name = inputs.data.get("license_name").unwrap().as_str().unwrap();
        let upstream_url = inputs.data.get("upstream_url").unwrap().as_str().unwrap();

        let output = convert_with_options(
            &Input {
                font: &font_bytes,
                sizes: &sizes,
                license: &license_bytes,
                upstream_url,
                require_chars: require_text.as_deref(),
            },
            &ConversionOptions { license_name },
        )
        .map_err(|e| format!("conversion error: {e}"))?;

        if !output.missing.is_empty() {
            return Err("required characters missing from font".into());
        }

        let mut files_map = BTreeMap::new();
        for f in &output.files {
            let p = packs.join(&f.name);
            fs::write(&p, &f.bytes)?;
            files_map.insert(f.name.clone(), JsonValue::String(sha256_bytes(&f.bytes)));
        }

        let mut stamp_map = identity_map;
        stamp_map.insert("format".to_string(), JsonValue::String(FORMAT.to_string()));
        stamp_map.insert("cache_key".to_string(), JsonValue::String(key.clone()));
        stamp_map.insert("files".to_string(), JsonValue::Object(files_map));

        let stamp_val = JsonValue::Object(stamp_map);
        fs::write(packs.join(STAMP), stamp_val.to_pretty_string(0) + "\n")?;

        if artifact.exists() {
            fs::remove_dir_all(&artifact)?;
        }
        fs::rename(packs, &artifact)?;
    }

    fs::create_dir_all(dest_parent)?;
    let temp_install = TempDir::new_in(dest_parent, "font-install-")?;
    let staged = temp_install.path().join("FONTS");
    fs::create_dir_all(&staged)?;

    // Copy artifact to staged
    for entry in fs::read_dir(&artifact)? {
        let entry = entry?;
        let name = entry.file_name();
        fs::copy(entry.path(), staged.join(&name))?;
    }

    if !is_verified_artifact(&staged, &key) {
        return Err("artifact changed while copying".into());
    }

    let backup = temp_install.path().join("previous");
    if destination.exists() {
        fs::rename(&destination, &backup)?;
    }
    fs::rename(staged, &destination)?;

    let font_name = inputs.data.get("name").unwrap().as_str().unwrap();
    let font_ver = inputs.data.get("version").unwrap().as_str().unwrap();
    println!(
        "{}: {} {} -> {}",
        if hit { "cache hit" } else { "built" },
        font_name,
        font_ver,
        destination.display()
    );

    Ok(hit)
}
