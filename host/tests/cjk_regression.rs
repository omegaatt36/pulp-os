// Consolidated CJK layout and typography regression test suite
// Covers historical layout bugfixes, metadata failure handling, style edge cases, and cache behavior.

pub mod cjk_support;

#[path = "cjk_regression/cache_reuse.rs"]
mod cache_reuse;
#[path = "cjk_regression/capacity.rs"]
mod capacity;
#[path = "cjk_regression/chapter_identity.rs"]
mod chapter_identity;
#[path = "cjk_regression/heading_latin_tail.rs"]
mod heading_latin_tail;
#[path = "cjk_regression/heading_pages.rs"]
mod heading_pages;
#[path = "cjk_regression/identity.rs"]
mod identity;
#[path = "cjk_regression/lifecycle.rs"]
mod lifecycle;
#[path = "cjk_regression/long_group.rs"]
mod long_group;
#[path = "cjk_regression/metadata_failure.rs"]
mod metadata_failure;
#[path = "cjk_regression/nested_styles.rs"]
mod nested_styles;
#[path = "cjk_regression/sd_reads.rs"]
mod sd_reads;
#[path = "cjk_regression/slicing.rs"]
mod slicing;
#[path = "cjk_regression/stage_prefix.rs"]
mod stage_prefix;
#[path = "cjk_regression/surface_fixes.rs"]
mod surface_fixes;
#[path = "cjk_regression/unused_bank.rs"]
mod unused_bank;
