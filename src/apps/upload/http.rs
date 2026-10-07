// HTTP layer of the upload app: GET / and /files, POST /upload and /delete
// over any async byte stream. No networking or hardware types here; the
// caller owns accept, timeouts and closing the connection.

use embedded_io_async::{Read, Write};

use crate::drivers::dir_entry::DirEntry;
use crate::drivers::sdcard::SdStorage;
use crate::drivers::storage;

const HTTP_200_HTML: &[u8] =
    b"HTTP/1.0 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n";
const HTTP_200_JSON: &[u8] =
    b"HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n";
const HTTP_200_TEXT: &[u8] =
    b"HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n";
const HTTP_500_TEXT: &[u8] =
    b"HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n";
const HTTP_404: &[u8] = b"HTTP/1.0 404 Not Found\r\nConnection: close\r\n\r\nNot Found";
const HTTP_431: &[u8] = b"HTTP/1.0 431 Headers Too Large\r\n\r\n";

const UPLOAD_PAGE: &[u8] = include_bytes!("../../../assets/upload.html");

const MAX_BOUNDARY_LEN: usize = 120;
const WORK_BUF_SIZE: usize = 2048;

const HTTP_HEADER_BUF_SIZE: usize = 1024;

const DIR_LIST_MAX: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerEvent {
    Nothing,
    Uploaded { name: [u8; 13], name_len: u8 },
    UploadFailed,
    Deleted { name: [u8; 13], name_len: u8 },
    DeleteFailed,
}

/// Serve the one HTTP request on an accepted connection: parse it, act on
/// the SD card, write the whole response and flush. The connection is left
/// open; the caller closes it.
pub async fn serve_request<S>(socket: &mut S, sd: &SdStorage) -> ServerEvent
where
    S: Read + Write,
{
    let mut hdr = [0u8; HTTP_HEADER_BUF_SIZE];
    let mut hdr_len = 0usize;

    loop {
        match socket.read(&mut hdr[hdr_len..]).await {
            Ok(0) | Err(_) => return ServerEvent::Nothing,
            Ok(n) => {
                hdr_len += n;
                if find_subsequence(&hdr[..hdr_len], b"\r\n\r\n").is_some() {
                    break;
                }
                if hdr_len >= hdr.len() {
                    let _ = socket.write_all(HTTP_431).await;
                    let _ = socket.flush().await;
                    return ServerEvent::Nothing;
                }
            }
        }
    }

    let Some(headers_end) = find_subsequence(&hdr[..hdr_len], b"\r\n\r\n") else {
        return ServerEvent::Nothing;
    };
    let body_offset = headers_end + 4;
    let initial_body = &hdr[body_offset..hdr_len];
    let headers = &hdr[..headers_end];

    let first_line_end = headers
        .iter()
        .position(|&b| b == b'\r')
        .unwrap_or(headers.len());
    let request_line = &headers[..first_line_end];

    let is_get = request_line.starts_with(b"GET ");
    let is_post = request_line.starts_with(b"POST ");

    let path = extract_path(request_line);

    if is_get && path == b"/" {
        let _ = socket.write_all(HTTP_200_HTML).await;
        let _ = socket.write_all(UPLOAD_PAGE).await;
        let _ = socket.flush().await;
        return ServerEvent::Nothing;
    }

    if is_get && path == b"/files" {
        // list first: a card that is missing or cannot be read must not look
        // like an empty one, so nothing is written until the listing is known
        let mut entries = [DirEntry::EMPTY; DIR_LIST_MAX];
        let count = match storage::list_root_files(sd, &mut entries) {
            Ok(n) => n,
            Err(e) => {
                log::info!("upload: listing failed: {}", e);
                send_error_response(socket, "list failed").await;
                return ServerEvent::Nothing;
            }
        };

        let _ = socket.write_all(HTTP_200_JSON).await;
        let _ = socket.write_all(b"[").await;
        let mut json_buf = [0u8; 80]; // per-entry scratch: {"name":"XXXXXXXX.XXX","size":4294967295}
        for (i, e) in entries.iter().enumerate().take(count) {
            let name = e.name_str();
            let mut pos = 0usize;
            let prefix = b"{\"name\":\"";
            json_buf[..prefix.len()].copy_from_slice(prefix);
            pos += prefix.len();
            let nb = name.as_bytes();
            json_buf[pos..pos + nb.len()].copy_from_slice(nb);
            pos += nb.len();
            let mid = b"\",\"size\":";
            json_buf[pos..pos + mid.len()].copy_from_slice(mid);
            pos += mid.len();

            pos += fmt_u32(e.size, &mut json_buf[pos..]);
            json_buf[pos] = b'}';
            pos += 1;
            if i + 1 < count {
                json_buf[pos] = b',';
                pos += 1;
            }
            let _ = socket.write_all(&json_buf[..pos]).await;
        }
        let _ = socket.write_all(b"]").await;
        let _ = socket.flush().await;
        return ServerEvent::Nothing;
    }

    if is_post && path == b"/upload" {
        let Some(boundary) = find_boundary(headers) else {
            send_error_response(socket, "Missing multipart boundary").await;
            return ServerEvent::UploadFailed;
        };

        return match handle_upload(socket, sd, boundary, initial_body).await {
            Ok((name, name_len)) => {
                let _ = socket.write_all(HTTP_200_TEXT).await;
                let _ = socket.write_all(b"OK").await;
                let _ = socket.flush().await;
                ServerEvent::Uploaded { name, name_len }
            }
            Err(e) => {
                log::info!("upload: handle_upload error: {}", e);
                send_error_response(socket, e).await;
                ServerEvent::UploadFailed
            }
        };
    }

    if is_post && path == b"/delete" {
        // the body is the plain file name, exactly Content-Length bytes
        let max_body = extract_content_length(headers).unwrap_or(0);
        let mut body = [0u8; 13];
        if max_body > body.len() {
            send_error_response(socket, "Invalid filename").await;
            return ServerEvent::DeleteFailed;
        }
        let have = initial_body.len().min(max_body);
        body[..have].copy_from_slice(&initial_body[..have]);
        let mut body_len = have;

        while body_len < max_body {
            match socket.read(&mut body[body_len..max_body]).await {
                Ok(n) if n > 0 => body_len += n,
                _ => break,
            }
        }

        if body_len < max_body {
            send_error_response(socket, "Truncated body").await;
            return ServerEvent::DeleteFailed;
        }

        let name = match core::str::from_utf8(&body[..body_len]) {
            Ok(s) => s,
            Err(_) => {
                send_error_response(socket, "Invalid filename").await;
                return ServerEvent::DeleteFailed;
            }
        };

        if !is_root_fat_83(name.as_bytes()) {
            send_error_response(socket, "Invalid filename").await;
            return ServerEvent::DeleteFailed;
        }

        let mut name_buf = [0u8; 13];
        let name_bytes = name.as_bytes();
        name_buf[..name_bytes.len()].copy_from_slice(name_bytes);
        let name_len = name_bytes.len() as u8;

        return match storage::delete_file(sd, name) {
            Ok(()) => {
                let _ = socket.write_all(HTTP_200_TEXT).await;
                let _ = socket.write_all(b"OK").await;
                let _ = socket.flush().await;
                ServerEvent::Deleted {
                    name: name_buf,
                    name_len,
                }
            }
            Err(e) => {
                log::info!("upload: delete failed for '{}': {}", name, e);
                send_error_response(socket, "delete failed").await;
                ServerEvent::DeleteFailed
            }
        };
    }

    let _ = socket.write_all(HTTP_404).await;
    let _ = socket.flush().await;
    ServerEvent::Nothing
}

async fn handle_upload<S>(
    socket: &mut S,
    sd: &SdStorage,
    boundary: &[u8],
    initial_body: &[u8],
) -> Result<([u8; 13], u8), &'static str>
where
    S: Read + Write,
{
    if boundary.len() > MAX_BOUNDARY_LEN {
        return Err("boundary too long");
    }

    let em_len = 4 + boundary.len();
    let mut end_marker_buf = [0u8; MAX_BOUNDARY_LEN + 4];
    end_marker_buf[0] = b'\r';
    end_marker_buf[1] = b'\n';
    end_marker_buf[2] = b'-';
    end_marker_buf[3] = b'-';
    end_marker_buf[4..em_len].copy_from_slice(boundary);
    let end_marker = &end_marker_buf[..em_len];

    let mut work = [0u8; WORK_BUF_SIZE];
    let init_len = initial_body.len().min(work.len());
    work[..init_len].copy_from_slice(&initial_body[..init_len]);
    let mut filled = init_len;

    let (file_name_buf, file_name_len) = loop {
        if let Some(pos) = find_subsequence(&work[..filled], b"\r\n\r\n") {
            let part_headers = &work[..pos];

            let raw_name = extract_filename(part_headers).ok_or("no filename in upload")?;
            let (name_buf, name_len) = sanitize_83(raw_name);
            if name_len == 0 {
                return Err("invalid filename");
            }

            // warn if sanitisation changed the name, two different
            // original names can map to the same 8.3 name, causing
            // the second upload to silently overwrite the first.
            if raw_name != &name_buf[..name_len as usize] {
                log::warn!(
                    "upload: sanitised '{}' -> '{}' (may overwrite existing file)",
                    core::str::from_utf8(raw_name).unwrap_or("?"),
                    core::str::from_utf8(&name_buf[..name_len as usize]).unwrap_or("?"),
                );
            }

            let file_start = pos + 4;
            work.copy_within(file_start..filled, 0);
            filled -= file_start;

            break (name_buf, name_len);
        }

        if filled >= work.len() {
            return Err("part headers too large");
        }

        let n = socket
            .read(&mut work[filled..])
            .await
            .map_err(|_| "read error")?;
        if n == 0 {
            return Err("connection closed during headers");
        }
        filled += n;
    };

    let name_str = core::str::from_utf8(&file_name_buf[..file_name_len as usize])
        .map_err(|_| "filename encoding error")?;

    log::info!("upload: receiving file '{}'", name_str);

    storage::write_file(sd, name_str, &[]).map_err(|_| "write failed")?;

    // holdback last end_marker.len() bytes to detect boundary spanning two reads

    let mut total_written: u32 = 0;

    loop {
        if let Some(pos) = find_subsequence(&work[..filled], end_marker) {
            if pos > 0 {
                storage::append_root_file(sd, name_str, &work[..pos])
                    .map_err(|_| "write failed")?;
                total_written += pos as u32;
            }
            log::info!("upload: complete, {} bytes written", total_written);
            return Ok((file_name_buf, file_name_len));
        }

        if filled > end_marker.len() {
            let safe = filled - end_marker.len();
            storage::append_root_file(sd, name_str, &work[..safe]).map_err(|_| "write failed")?;
            total_written += safe as u32;

            work.copy_within(safe..filled, 0);
            filled = end_marker.len();
        }

        let n = socket
            .read(&mut work[filled..])
            .await
            .map_err(|_| "read error during upload")?;
        if n == 0 {
            if filled > 0 {
                let _ = storage::append_root_file(sd, name_str, &work[..filled]);
            }
            return Err("upload incomplete");
        }
        filled += n;
    }
}

fn extract_path(line: &[u8]) -> &[u8] {
    let start = match line.iter().position(|&b| b == b' ') {
        Some(p) => p + 1,
        None => return b"/",
    };

    let rest = &line[start..];
    let end = rest.iter().position(|&b| b == b' ').unwrap_or(rest.len());

    let path = &rest[..end];
    let qmark = path.iter().position(|&b| b == b'?').unwrap_or(path.len());
    &path[..qmark]
}

fn find_boundary(headers: &[u8]) -> Option<&[u8]> {
    let marker = b"boundary=";
    let pos = headers
        .windows(marker.len())
        .position(|w| w.eq_ignore_ascii_case(marker))?;
    let start = pos + marker.len();
    let rest = &headers[start..];

    if rest.is_empty() {
        return None;
    }

    if rest[0] == b'"' {
        let inner = &rest[1..];
        let end = inner.iter().position(|&b| b == b'"')?;
        if end == 0 {
            return None;
        }
        Some(&inner[..end])
    } else {
        let end = rest
            .iter()
            .position(|&b| b == b'\r' || b == b'\n' || b == b';' || b == b' ')
            .unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        Some(&rest[..end])
    }
}

fn extract_filename(headers: &[u8]) -> Option<&[u8]> {
    let marker = b"filename=\"";
    let pos = headers
        .windows(marker.len())
        .position(|w| w.eq_ignore_ascii_case(marker))?;
    let start = pos + marker.len();
    let rest = &headers[start..];
    let end = rest.iter().position(|&b| b == b'"')?;
    if end == 0 {
        return None;
    }
    Some(&rest[..end])
}

fn sanitize_83(raw: &[u8]) -> ([u8; 13], u8) {
    let name = match raw.iter().rposition(|&b| b == b'/' || b == b'\\') {
        Some(p) => &raw[p + 1..],
        None => raw,
    };

    let (base_src, ext_src) = match name.iter().rposition(|&b| b == b'.') {
        Some(dot) => (&name[..dot], &name[dot + 1..]),
        None => (name, &[] as &[u8]),
    };

    let mut out = [0u8; 13];
    let mut pos: usize = 0;

    for &b in base_src.iter() {
        if pos >= 8 {
            break;
        }
        if is_valid_83_char(b) {
            out[pos] = b.to_ascii_uppercase();
            pos += 1;
        }
    }

    if pos == 0 {
        out[..6].copy_from_slice(b"UPLOAD");
        pos = 6;
    }

    if !ext_src.is_empty() {
        out[pos] = b'.';
        pos += 1;
        let ext_start = pos;
        for &b in ext_src.iter() {
            if pos - ext_start >= 3 {
                break;
            }
            if is_valid_83_char(b) {
                out[pos] = b.to_ascii_uppercase();
                pos += 1;
            }
        }

        if pos == ext_start {
            pos -= 1;
        }
    }

    (out, pos as u8)
}

// Deletion accepts existing FAT short names without applying the upload
// sanitizer. Require the exact root basename: no path or whitespace cleanup,
// and a dot only between a 1-8 byte base and a 1-3 byte extension.
fn is_root_fat_83(raw: &[u8]) -> bool {
    let (base, ext) = match raw.iter().position(|&b| b == b'.') {
        Some(dot) => (&raw[..dot], Some(&raw[dot + 1..])),
        None => (raw, None),
    };
    (1..=8).contains(&base.len())
        && ext.is_none_or(|ext| (1..=3).contains(&ext.len()))
        && base.iter().chain(ext.unwrap_or(&[])).all(|&b| {
            (b'!'..=b'~').contains(&b)
                && !matches!(
                    b,
                    b'"' | b'*'
                        | b'+'
                        | b','
                        | b'.'
                        | b'/'
                        | b':'
                        | b';'
                        | b'<'
                        | b'='
                        | b'>'
                        | b'?'
                        | b'['
                        | b'\\'
                        | b']'
                        | b'|'
                )
        })
}

fn is_valid_83_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'~' | b'!' | b'#' | b'$' | b'&')
}

fn fmt_u32(mut n: u32, buf: &mut [u8]) -> usize {
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 10];
    let mut pos = 0;
    while n > 0 {
        tmp[pos] = b'0' + (n % 10) as u8;
        n /= 10;
        pos += 1;
    }
    for i in 0..pos {
        buf[i] = tmp[pos - 1 - i];
    }
    pos
}

fn extract_content_length(headers: &[u8]) -> Option<usize> {
    let marker = b"content-length:";
    let pos = headers
        .windows(marker.len())
        .position(|w| w.eq_ignore_ascii_case(marker))?;
    let start = pos + marker.len();
    let rest = &headers[start..];

    let trimmed = rest.iter().position(|&b| b != b' ' && b != b'\t')?;
    let rest = &rest[trimmed..];
    let end = rest
        .iter()
        .position(|&b| b == b'\r' || b == b'\n')
        .unwrap_or(rest.len());
    let digits = &rest[..end];
    let mut val: usize = 0;
    for &b in digits {
        if b.is_ascii_digit() {
            val = val.saturating_mul(10).saturating_add((b - b'0') as usize);
        } else {
            break;
        }
    }
    Some(val)
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn send_error_response<S: Write>(socket: &mut S, msg: &str) {
    let _ = socket.write_all(HTTP_500_TEXT).await;
    let _ = socket.write_all(msg.as_bytes()).await;
    let _ = socket.flush().await;
}
