# Pagination identity independent tests

Authored `host/tests/cjk_identity.rs` before pagination identity implementation.
Read the task contract, source-only cache map, frozen `cjk_support`, existing
public host interfaces and representative existing tests. No production method
bodies, reference pagination implementation or mutants were read or written.
Only production callback signature lines were queried to add approved host
forwarders. Production sources were not edited.

## Requirement-derived oracle

- The literal PFNT fixture has 17-pixel body advances at 16px and 24-pixel heading
  advances at 23px. Widths 68 and 96 therefore admit four scalars per line.
- Replacement bytes use different literal font IDs, unchanged pixel sizes,
  advances 34 and 48, and bitmap rows `[0x42, 0xA5, 0x5A]`. These widths admit two
  scalars. No wrapping implementation determines those expectations.
- Live size cycling to index 1 selects the frozen 19px body fixture with literal
  advance 20. Width 68 admits three scalars. Its literal bitmap fingerprint is
  checked through the frozen framebuffer observation helper.
- A captured raw anchor must fall within the newly displayed page's byte span.
  Page numbers need not match, and the rebuilt page may start before the anchor.
  The displayed source slice must agree exactly and UTF-8 offsets must be scalar
  boundaries. Heading markers are kept in the source so raw offsets retain their
  two-byte prefix; the nonzero test anchor lies well inside the heading.
- Forward/back must display identical content from the rebuilt raw page start.
  Complete TXT navigation must concatenate to the literal source exactly once.
  This is a text-conservation invariant, not a reference pagination algorithm.
  The frozen punctuation observation helper is also applied to displayed lines.
- A bookmark saved under the old pack is reopened in a fresh Rig after replacing
  the pack, with anchor containment, new bitmap, navigation and conservation.

The body/heading replacement tests independently exercise each active bank.
The bookmark test is a passing regression control for existing raw-byte restore.
The compiled layout-version requirement has no runtime change control by design;
verify its presence and use in the production identity by source review. These
tests do not introduce a persistent page-cache file or inspect EPUB byte caches.

## Host controls

Added only `Rig::resume()` and `Rig::quick_cycle(id, value)` in
`host/src/reader.rs`. Each directly invokes the actual App callback, reapplies
the existing width override and runs normal background progression. No
pagination, invalidation, glyph preparation or anchor logic exists in them.
`resume()` invokes the retained reader's resume callback directly; no unrelated
session control was added.

## Actual RED

Command, run alone after receiving the root agent's cargo slot:

```sh
cargo test -p pulp-host --target aarch64-apple-darwin --config 'unstable.build-std=["std","test"]' --test cjk_identity -- --test-threads=1
```

The command completed with exit 101 after successful compilation. Four tests ran:

| Test | Actual baseline result |
|---|---|
| Same-size body replacement resume | FAIL: old four-scalar line retained where replacement permits two |
| Same-size heading replacement resume | FAIL: old four-scalar line retained where replacement permits two |
| Live size-cycle raw-anchor/navigation | FAIL: full forward/back walk omits source scalars |
| Bookmark reboot with replaced pack | PASS: existing raw-byte restore is preserved |

The failures are behavioral assertions, not compile errors, injected failures or
environment problems. No broad suite or mutant was run. Formatting used only
`rustfmt --edition 2024 host/tests/cjk_identity.rs`.

Original RED log: `/private/tmp/t9-identity-red.log`.
Frozen RED log: `/private/tmp/cjk-snap/T9-tests/red.log`.

## Frozen artifacts and weakening gate

Snapshot: `/private/tmp/cjk-snap/T9-tests`.
Manifest: `tmp-briefs/t9-tests-manifest.txt` and snapshot `SHA256SUMS`.
Test SHA256: `42376f2733b6e7051735a008214eba869495428d13b262bb8d64b7b2f6eef1a3`.
Fixture support SHA256: `c54f7693bfaec43d1c621ac935f79993b43b91c00e6c1b7ed034bb33a81d7393`.

The host file is shared with other agents, so only the exact added forwarding
block is frozen as `rig-forwarding.rs.txt`; no hash claim covers unrelated host
edits. Fixture support remains unchanged and read-only.

Implementation must pass the frozen tests unchanged. Do not relax cell-count,
bitmap, anchor-containment, scalar-boundary, navigation or exact-conservation
assertions to obtain GREEN. A justified fixture/oracle correction requires a
separate independent test-author review and recaptured RED plus updated manifest
before implementation continues. No production edits or commits were made by
this test author.
