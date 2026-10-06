# T7 test-author report

Authored eight focused behavior tests in host/tests/cjk_reader.rs and literal
fixtures in host/tests/cjk_support/mod.rs. Requirements and host geometry seam
are in t7-contract.md. No forthcoming T7 implementation was read or written.
No firmware source, existing tests, mutations, reference implementation or
commits were added. At controller request, host/src/reader.rs retains a width
override and host/src/apps/probe.rs sets the existing ReaderApp text_w field.
Those host additions contain no wrapping or pagination algorithm.

## RED evidence

Actual command (2026-10-06):

```sh
cargo test -p pulp-host --target aarch64-apple-darwin \
  --config 'unstable.build-std=["std","test"]' \
  --test cjk_reader -- --test-threads=1
```

Before the authorized host addition: exit101, two E0599 missing set_text_width
errors, no warnings. After the addition: compiled successfully, exit101,
0 passed / 8 failed / 0 ignored; no warnings. Firmware remained unchanged.

| Test | Observed missing behavior |
|---|---|
| absent_scalar_draws_the_explicit_hollow_box_instead_of_question_mark | Literal 17x17 box absent at x11 |
| body_sizes_measure_and_draw_the_selected_pack_including_supplementary_text | 16px actual one line 臺灣𠮷; expected 臺灣 / 𠮷 |
| heading_uses_its_size_pack_without_replacing_latin_font_styles | 23px heading actual one line; expected two |
| inseparable_punctuation_at_tiny_width_makes_progress_without_losing_text | Forbidden line tail 臺（ |
| metrics_precede_wrap_but_only_visible_bitmaps_are_loaded_and_render_reads_nothing | No visible pack bitmap read |
| narrow_mixed_lines_obey_both_punctuation_sets_and_conserve_scalars | Forbidden line head ，𠮷。L |
| page_walk_back_and_bookmark_restore_keep_the_same_text_position | Forbidden line head 。臺灣「繁體中文」， |
| required_pack_failures_are_recoverable_and_never_publish_invisible_ready_text | Absent required pack publishes Ready, expected Error |

Some assertions follow earlier failing assertions; these RED results do not
claim all branches executed. Replay after implementation must run the full
eight tests. Full host suite belongs to the controller and was not rerun here.

## Frozen weakening-gate baseline

Snapshot: /private/tmp/cjk-snap/T7-tests (also accessible as /tmp/cjk-snap/T7-tests).
Files are mode0444; directories mode0555. Includes both new test files, both
host harness files, and t7-contract.md. Existing unrelated changes in harness
files are preserved. SHA256SUMS is included in that snapshot.

Repository manifest: tmp-briefs/t7-tests.sha256.
Manifest SHA-256: a7372acae643d71c662dda0b51071243ec66a4f6d18df59ed313f7c6f886e3a9.

Controller must compare frozen tests byte-for-byte before accepting GREEN.
Harness files may need implementation wiring; review their deltas separately
from frozen expectations. The contract/test oracles use literal byte format,
literal advances/bitmaps, explicit punctuation sets and source text; production
outputs do not define expected metrics. Pixel placement observations use the
existing host geometry, with a literal glyph patch checked within its line band.

## Review handoff

Self-review checked hand-written header44/record22 structure, sorted scalar
records, bitmap lengths/padding, per-size and per-scalar pixel fingerprints,
the >u8 advance case, positive widths within screen bounds and fixture length.
No behavior blocker found in test design. Controller independent code review
and actual production GREEN remain required; this report claims tests-only RED.
