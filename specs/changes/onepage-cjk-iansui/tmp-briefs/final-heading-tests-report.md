# Heading Latin-tail tests report

File: host/tests/cjk_heading_latin_tail.rs
sha256 (final, see Update below): feec63bd322472411f1925b3f827c510c19101145156aa5bc8a577ce4cefdf18
sha256 (original, red-proof run): 79ee5137dd290e592148b2668a989f9a15a1d554443dc935df4965cff77570df

Command: cargo test -p pulp-host --target <host> --config 'unstable.build-std=["std","test"]' --test cjk_heading_latin_tail
Result against current production: 3 failed, 2 passed. No compile errors. No other file was touched.

## Observable choice
Static Latin glyphs are wider in heading style than in body style, so the number of Latin chars that fit a
48px line distinguishes the styles. Reference capacities come from two oracle rigs measured on page 0 only:
heading-only Latin (`\x01H` + A*600 + `\x01h`) and body-only Latin (A*600). Page-0 measurement gives
heading = 2 chars/line, body = 4 chars/line. Fixture sanity assert: heading < body.
Which cap is heading is derived from the requirement (page 0 is inside the open heading). The numeric values
2 and 4 are measured from the page-0 oracles, not hard-coded. The helper functions of cjk_support cannot
read the Latin font size directly, so the chars-per-line observable is used. Pack metrics (advance px+1,
rows(px,c)) are used only for the CJK drawn-size assertions.

## Tests and failing output

1. latin_only_pages_inside_open_cjk_heading_keep_heading_style - FAILS
   Input: `\x01H` 臺 A*600 `\x01h\nBody.`, width 48. Every page that starts and ends before the closing marker
   (excluding page 0) must have all non-last lines at heading capacity (2 chars/line); at least 3 such pages.
   ```
   panicked at host/tests/cjk_heading_latin_tail.rs:83:9:
   assertion `left == right` failed: page 1 at raw offset 74: line 0 "AAAA" not in heading style
     left: 4
    right: 2
   ```
   Reason: page 1 (raw offset 74, before the closing marker at 604) renders Latin in body width. This is the stated defect.

2. navigation_and_bookmark_restore_keep_heading_style_on_latin_only_page - FAILS
   Same input; Next, Next, Prev, save/flush/reboot/restore, then Next. Page lines must be identical after
   Prev and restore (as in the existing test), and the Latin capacity must be heading at each step.
   ```
   panicked at host/tests/cjk_heading_latin_tail.rs:83:9:
   assertion `left == right` failed: page 1 first visit: line 0 "AAAA" not in heading style
     left: 4
    right: 2
   ```
   Fails on the first visit to page 1, so the Prev and restore steps are not reached today (not independently red-proven).
   Consistency-only comparisons (`lines()` equal) would pass with the defect; the capacity asserts are what bind style.

3. oracle_latin_heading_is_consistent_across_pages - FAILS (broader finding)
   A heading that starts with Latin only (no CJK at all): `\x01H` A*600 `\x01h\n`.
   ```
   panicked at host/tests/cjk_heading_latin_tail.rs:83:9:
   assertion `left == right` failed: oracle: line 0 "AAAA" not in heading style
     left: 4
    right: 2
   ```
   So the defect is not limited to CJK-start headings: any Latin-only continuation page of an open heading drops to
   body style. The page-0 capacity oracle is unaffected (page 0 is correct). Controller should decide whether
   to keep this test or fold it into 1; it asserts the same requirement text.

4. late_cjk_in_open_heading_is_drawn_at_heading_size_after_latin_only_pages - PASSES today
   Input: `\x01H` 臺 A*600 臺*400 `\x01h\n...`. A page that is entirely 臺, starts in the tail and lies inside the
   heading must draw rows(23,'臺') at line 0, after at least 2 Latin-only pages. It passes, so it does not
   capture the defect. Probe evidence (scratch, removed): with this shape the Latin-only pages themselves
   also keep heading width (the CJK later in the text appears to keep the style), i.e. the CJK-after-Latin
   fallback in the brief cannot fail today. It stays as a regression guard, not weakened.

5. body_style_resumes_after_closing_marker - PASSES today
   Input: heading 臺 A*600 `\x01h\n` A*600 `\n` 臺*400. Checks: >=3 heading pages before the close at heading
   capacity (2), pages wholly after the close at body capacity (4), later pure-CJK pages at rows(16,'臺').
   Passes today for the same reason as test 4: scratch print showed pages 1-7 at 2 chars/line here, because
   CJK text follows later in the file. Same Latin run as test 1 stays heading-width when a CJK run follows.
   The after-close half (requirement item 3) holds today and is a guard. Do not read this test as evidence
   that item 1 works.

## Trigger observed by scratch probes (not committed)
Heading `\x01H` 臺 A*n `\x01h\n` B*tail with no CJK after the close: pages 1..last are all 4 chars/line (body
width) for n in {200,600,1500,3000} and tail in {0,5,20,200,700}; page 0 is 2. With a CJK run later in the
file (tests 4, 5) the same pages are 2 chars/line. So the loss occurs when no CJK exists after the page
start (consistent with "later pages contain ONLY Latin"). Tests 1-3 intentionally have no CJK after the
heading start.

## Expected-value derivation
- Heading Latin capacity: measured on page 0 of a heading-only rig (heading style is unambiguously active
  there per the requirement). Not copied from the failing pages.
- Body Latin capacity: measured on a rig with no markers.
- CJK glyph px: 23 for heading size-index 0 (cjk_support HEADING[0]), 16 for body (BODY[0]); bitmap rows(px,c).
- Page eligibility: raw offsets from `page_offsets()` compared with the byte offset of the closing marker
  computed from the literal input.
- Last line of each page is excluded from the capacity check because a page end can be a partial line.

## Not done
- Fixed behavior was not exercised (no production edits allowed), so tests 1-3 are red-proven only; the green
  side is unconfirmed beyond tests 4 and 5 passing.

## Update 2026-10-06: spec decision, test 3 replaced (weakening-gate disclosure)
User-approved spec change: R10 heading persistence across Latin-only continuation pages applies only once a CJK
glyph window has appeared earlier in the chapter. Pure-English chapters keep original behaviour (style resets at
every page start, pinned by the English golden). An assertion was therefore INVERTED by spec decision:
`oracle_latin_heading_is_consistent_across_pages` (continuation pages expected at heading capacity) became
`pure_latin_heading_resets_style_at_page_start` (continuation pages expected at body capacity, derived from the
body reference rig `caps().1`, which is measured on a marker-free Latin rig, not from the failing page). Page 0
is still asserted at heading capacity. The "broader finding" and "Trigger observed" notes above are superseded
by this decision; tests 1, 2, 4, 5 are unchanged. The earlier "FAILS" output in this report is from the
pre-fix production run (sha 79ee5137...).

Exact diff of the replaced test:
```diff
--- /tmp/before.rs	2026-10-06 20:50:39
+++ host/tests/cjk_heading_latin_tail.rs	2026-10-06 20:50:46
@@ -89,19 +89,28 @@
 }
 
 #[test]
-fn oracle_latin_heading_is_consistent_across_pages() {
-    // Fixture sanity: a Latin-start heading keeps its width on every page.
-    let (h, _) = caps();
+fn pure_latin_heading_resets_style_at_page_start() {
+    // Characterization (spec R10, scoped): with no CJK glyph window earlier in the chapter,
+    // heading style is reset at every page start, as in the original English behaviour.
+    let (h, b) = caps();
     let mut input = vec![1, b'H'];
     input.extend_from_slice("A".repeat(LATIN).as_bytes());
     let end = input.len() as u32;
     input.extend_from_slice(b"\x01h\n");
     let mut r = rig(&input, 0, WIDTH);
+    assert_eq!(
+        chars_per_line(&text_lines(&r)[0]),
+        h,
+        "page 0 opens the heading"
+    );
     let mut pages = 0;
     walk(&mut r, |r, p| {
-        if r.page_offsets()[p] < end && r.page_offsets().get(p + 1).is_some_and(|&o| o <= end) {
+        if p > 0
+            && r.page_offsets()[p] < end
+            && r.page_offsets().get(p + 1).is_some_and(|&o| o <= end)
+        {
             pages += 1;
-            assert_latin_page_capacity(r, h, "oracle");
+            assert_latin_page_capacity(r, b, &format!("pure-Latin continuation page {p}"));
         }
     });
     assert!(pages >= 3);
```

Run against current production (after the production fix):
```
running 5 tests
test pure_latin_heading_resets_style_at_page_start ... ok
test navigation_and_bookmark_restore_keep_heading_style_on_latin_only_page ... ok
test latin_only_pages_inside_open_cjk_heading_keep_heading_style ... ok
test late_cjk_in_open_heading_is_drawn_at_heading_size_after_latin_only_pages ... ok
test body_style_resumes_after_closing_marker ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```
New sha256: feec63bd322472411f1925b3f827c510c19101145156aa5bc8a577ce4cefdf18

## Update 2026-10-06 (2): drawn style of the closing-marker line
Sha256 before this update: feec63bd322472411f1925b3f827c510c19101145156aa5bc8a577ce4cefdf18
New sha256: 36e2d72cdd43660fa622d95b0430001258a328196fc847cf03f9f6e06cbab558
Edit scope: one added import (`pulp_host::render::render_full`) and three appended tests; the 5 earlier tests are unchanged.

Observable: pixel strip of one text line (text_margin..+48 px, full font_line_h rows) from `render_full(&|s| r.draw(s))`.
Expected value = strip of the same visible text drawn alone on line 0 of a reference rig, as heading
(`\x01H` + text + `\nA`, no closing marker) or as body (no markers). Fixture assert: heading and body reference
strips differ for the text. Heading vs body glyphs are visibly different (heading A is wider and taller),
so the observable is valid with the public Rig API.

Tests added:
- closing_marker_line_is_drawn_in_heading_style_on_latin_only_continuation_page: FAILS (red-proof)
  input `\x01H` 臺 A*600 `\x01h\nBody.`; marker page is page 8 (continuation), all heading lines 0..19 match the
  heading reference, line 20 (raw `[65, 1, 104]`, visible "A") does not.
  ```
  panicked at host/tests/cjk_heading_latin_tail.rs:319:13:
  single-letter run: page 8 line 20 "A" (marker line is 20) is not drawn in heading style
  ```
  Scratch ASCII dump of that line (not committed) showed the body-sized glyph "A" where the heading reference shows the larger heading "A".
- text_after_closing_marker_is_drawn_in_body_style: PASSES today (the "Body." line equals the body reference). Guard for "body resumes only after the marker".
- closing_marker_line_with_spaces_in_latin_words_is_drawn_in_heading_style: FAILS (red-proof)
  heading `\x01H` 臺 + "AAA BBB "*75 + suffix, for 9 suffixes ("", " ", "A", "A ", " B", "A B", "AA B", "A BB", "B A "); every
  non-blank line up to and including the marker line on the marker page is compared with its heading reference, and
  at least one variant must put a space on the marker line. First variant fails, others not reached:
  ```
  panicked at host/tests/cjk_heading_latin_tail.rs:319:13:
  suffix "": page 6 line 14 "BBB " (marker line is 14) is not drawn in heading style
  ```
  Only the marker line fails; lines 0..13 of that page match heading style. Without implementation knowledge I
  only guarantee line breaks fall on spaces (the word text), not which line the break lands on; the
  `with_space >= 1` assert guards that a variant actually puts a space on the marker line (the failing
  variant already does: "BBB "). Whether the other 8 suffixes also fail is unverified until the fix lands.

Full run on current production: 8 tests, 6 passed, 2 failed (the two tests above).
