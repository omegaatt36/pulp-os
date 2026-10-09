// Consolidated smol-epub integration test suite
// Covers entity decoding, gaps, malformed content, rig fixtures, streaming, TOC, and zip limits.

pub mod smol_common;

#[path = "smol_epub/entity.rs"]
mod entity;
#[path = "smol_epub/gaps.rs"]
mod gaps;
#[path = "smol_epub/malformed.rs"]
mod malformed;
#[path = "smol_epub/rig.rs"]
mod rig;
#[path = "smol_epub/stream.rs"]
mod stream;
#[path = "smol_epub/toc.rs"]
mod toc;
#[path = "smol_epub/zip_limit.rs"]
mod zip_limit;
