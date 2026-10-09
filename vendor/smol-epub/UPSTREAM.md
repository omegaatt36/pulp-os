# smol-epub (vendored)

Upstream: https://github.com/hansmrtn/smol-epub
Base revision: `832609d967d4d06452dd49d35b2c9aeb578612de` ("perf improvements"), taken from
`git archive` of that commit, not from a working tree.
License: MIT OR Apache-2.0 (`LICENSE`).

This copy is the only smol-epub the workspace builds (`smol-epub` in the root
`Cargo.toml` points here). The sibling checkout `../smol-epub` is not used.

Differences from the upstream tree besides the patches below: `.github/` and
`Cargo.lock` were removed (the workspace `Cargo.lock` governs).

## Local patches on top of the base revision

Every text smol-epub produces (chapter text, OPF title and creator, TOC labels,
`strip_html_inplace` output) is valid UTF-8. Malformed input becomes one U+FFFD
per maximal subpart, the same count as `String::from_utf8_lossy` and the same
policy as `decode_utf8_char` in `kernel/src/util/utf8.rs` (this crate does not
depend on the kernel; the ranges are written out again in `src/utf8.rs`).
Guarded by `host/tests/smol_malformed.rs`, `smol_entity.rs`, `smol_toc.rs`,
`smol_stream.rs` and `smol_rig.rs`.

- `src/utf8.rs` (new): `Utf8Fixer`, a byte-at-a-time maximal-subpart repair
  that keeps a sequence cut between calls in 8 bytes of state;
  `copy_sanitized` (repair + cut on a scalar boundary at a capacity) and
  `sanitize_inplace`. (D3)
- `src/lib.rs`: `mod utf8;`. (D3)
- `src/html_strip.rs`, `HtmlStripStream`: text bytes >= 0x80 go through
  `Utf8Fixer`; an ASCII byte or the end of input ends a sequence in progress
  with U+FFFD; `finish` no longer drops pending output when the buffer is
  short. Adds 8 bytes of stream state. (D3)
- `src/html_strip.rs`, numeric references: digits are folded into a `NumRef`
  (no buffer, any length, saturating); 0, surrogates, above U+10FFFF and
  overflow give one U+FFFD; `&#;`, `&#xZZ;` keep the literal-`&` recovery. The
  hex/decimal parsers that returned 0 on bad input and wrapped on overflow are
  gone. (D2)
- `src/html_strip.rs`, `strip_html_inplace`: repairs the buffer first
  (grows it only when replacements are longer than what they replace). (D3)
- `src/epub.rs`: OPF title / creator and NCX labels go through
  `copy_sanitized`; nav labels are built by `Label` (repair, whitespace
  collapse, cut after the last whole scalar that fits `TOC_TITLE_CAP`);
  `TocEntry` titles are no longer hard-cut at a byte count; `truncate_utf8`
  removed. (D1, D3)
- `src/zip.rs`, `src/async_io.rs` (`extract_deflate`, `extract_deflate_async`):
  `HasMoreOutput` is an error only when the call made no progress. The
  decoder reports it as soon as the output buffer is full, while the last
  input bytes (end of the final block) are still unread, which failed entries
  whose compressed size is 4096 k + 1 or + 2. (D4)
- `src/cache.rs`: comment on `strip_html_buf`'s reservation (output can be
  longer than the input).

Known limit, not fixed: the ZIP index keeps the first 256 entries and ignores
the rest (pinned by `host/tests/smol_zip_limit.rs`).
