# T9 page-cache identity map

Source-only map on 2026-10-06. No tests, builds or production/test edits.
T7 metadata-fix work may be concurrent; this map concerns layout identity only.

## Production state that can become incompatible

| State | Source | Current identity/invalidation |
|---|---|---|
| Raw page starts, count and completed-index flag | reader/mod.rs:194 PageState.offsets/total_pages/fully_indexed | No font identity, selected pixel sizes or layout geometry key. Existing offsets are reused on backward/forward loads. |
| Heading/emphasis/quote state at each page start | PageState.style_flags/indents | Derived from those same offsets; must be invalidated/rebuilt with the page table. |
| Current visible bytes/spans | PageState.buf/buf_len/lines/line_count | Held while Ready; regenerated on NeedPage, using whatever metrics are read then. |
| Next-page byte prefetch | PageState.prefetch/prefetch_page/prefetch_len | Tagged by page number, not font identity. Reset by reset_paging; bytes themselves remain valid text, but their start corresponds to the old offset table. |
| Staged CJK metrics | fonts/cjk.rs CjkState.metrics | Key is only (pixel_size,scalar); rebuilt every staged window. No persistent font_id retained. |
| Visible CJK bitmaps | CjkState.body/heading | Owners tagged by pixel size. Replaced on preparation; retained while Ready/suspended. PageCache internally stores FontInfo, including font_id, but CjkState does not compare or expose that identity. |
| Static Latin selection | Reader.fonts / book_font_size_idx / applied_font_idx | apply_font_metrics resolves Copy FontSet; applied_font_idx tracks only the selected size index. |

Replacing a pack at the same pixel size can therefore reload **new glyph metrics**
on a later NeedPage while still using **old PageState offsets**. TXT append-index
logic updates only when visiting its final known page and not fully_indexed;
it does not repair existing page starts. Cached EPUB preindex tables similarly
remain until NeedIndex/reset. If advances change, consuming a page from an old
start can disagree with the stored next start and cause overlap or skipped text.
This is the page-layout cache to key/invalidate, not the EPUB stripped-text cache:
epub.ch_cache, chapter_table and disk chapter cache hold font-independent bytes.

## Existing invalidation and anchor paths

- reader/paging.rs reset_paging resets page 0, offset 0, initial marker state,
  total_pages, fully_indexed, visible lengths, prefetch and page image.
- reader/mod.rs on_enter calls reset_paging and cjk.clear, then uses a session
  raw chapter/offset or bookmark restore_offset to scan from page 0.
- on_exit clears visible/CJK owners. on_suspend retains them.
- on_resume at reader/mod.rs:900 reapplies theme/metrics, but resets layout only
  when book_font_size_idx differs from applied_font_idx. A replacement font_id
  at the same size is invisible to that check. The current reset also does not
  first preserve the active raw anchor in restore_offset.
- on_quick_cycle_update at 1390 changes size, reapplies metrics, and sends cached
  EPUB to NeedIndex. TXT goes to NeedPage without resetting its offset table.
  This callback does not preserve/reseek the old raw anchor itself.
- byte_offset at 630 reports pending session/restore offset or current page start.
  save_position stores raw page offset+chapter. restore_state at 656 accepts a raw
  chapter/offset and size. Bookmark/session restore scans the new pagination to
  the page containing that offset; a changed layout need not start at exactly the
  old offset. Those are the existing raw-anchor seams for T9.

## Available public host controls

| Need | Existing control |
|---|---|
| Replace installed pack without replacing Rig | Rig::storage() + VirtualStorage::write_in_pulp_subdir("FONTS",file,bytes), or frozen cjk_support::install |
| Distinct font_id at unchanged pixel size | Hand pack header bytes 8..16 are little-endian u64 font_id; pixel size stays at 6..8. Frozen pack(px,wide) uses literal 0xABCD000000000000+px. Clone bytes in a new fixture; change ID and advances/bitmap fingerprints independently. Changing ID alone leaves pagination behavior unchanged. |
| Select size/theme before reopening | Rig::configure(book_font,theme); this forwards setters only and does not invoke resume/reindex while Ready. |
| Fixed narrow geometry | Rig::set_text_width(width), reapplied around ticks/open/input. |
| Navigate and observe stale/current table | Rig::press(Next/Prev), page/page_offsets/total_pages/fully_indexed/lines/line_infos and draw. |
| Reopen at saved raw anchor | save_position; open(BOOK), or save_position+bookmarks_flush+into_storage→Rig::new→configure→open. bookmark_save can seed an exact raw offset/chapter directly. |
| Observe backing source reads | storage read_log/read_count/reset_reads. |
| Invoke production on_resume or quick size-cycle callback | **No existing public Rig method.** quick_trigger forwards trigger callbacks, not on_quick_cycle_update. No public session restore_state wrapper exists either. A thin host-only forwarder would be needed for those exact production entry points. |

No test oracle or proposed implementation is supplied. A same-size replacement
followed by existing Next/Prev can expose incompatible offsets without a new
host seam; an exact suspend/resume or live size-change test needs the narrowly
forwarding controls identified above.
