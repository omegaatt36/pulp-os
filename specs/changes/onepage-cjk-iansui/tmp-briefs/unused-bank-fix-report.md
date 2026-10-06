# Unused fallback bank production repair

Production scope: `src/apps/reader/mod.rs`, `src/apps/reader/paging.rs`, additive `src/fonts/cjk.rs` bank-use query. Tests and existing oracles remain read-only. No commits, mutants, or source spec IDs. Source baseline: `/private/tmp/unused-bank-fix-start/{mod.rs,paging.rs,cjk.rs}`. Scoped diff: `unused-bank-fix.diff` (relative to that baseline, preserves prior agents' changes).

## Behavior

All text records body/heading pixel sizes and layout algorithm version before any wrapping path. Validated bank identities are separate optional fields. A bank becomes tracked only after staged fallback metrics depend on its pixel size. Pure Latin staging has no such metrics, so it performs no bank validation, preserving availability with absent, valid, or corrupt unused packs.

Tracked participating identities persist across subsequent Latin windows and release of transient rendering caches. Resume/background identity checks reopen only tracked banks, retaining same-size body and heading replacement detection. Identity mismatches continue through the existing raw-anchor invalidation/rebuild path. Size/layout changes retain the approved all-text rebuild policy. Corrupt banks still fail when actual fallback staging uses them.

## Verification

Frozen RED evidence comes from `unused-bank-tests-report.md`: three real failures against pre-repair production. Its test hash still matches `72d094e6b9b2acb6b04d492af20d7236440206aee4321a9bbc2185cb3bc64b25`.

Host command (exit 0):
```
cargo test --offline -p pulp-host --target aarch64-apple-darwin --config 'unstable.build-std=["std","test"]' --test cjk_unused_bank --test cjk_identity --test cjk_chapter_identity --test cjk_reader --test cjk_surfaces --test cjk_surface_fixes --test cjk_heading_pages --test cjk_nested_styles --test reader_epub --test reader_bookmarks
```
Log: `/private/tmp/unused-bank-fix-host.log`. 75 passed, 0 failed: unused bank 3; identity 4; chapter identity 1; reader 9; surfaces 7; surface fixes 3; heading pages 1; nested styles 1; EPUB 31; bookmarks 15.

Focused frozen English acceptance (exit 0):
```
CARGO_NET_OFFLINE=true READER_REG_WORKDIR=/private/tmp/unused-bank-fix-check bash scripts/check-reader-regression.sh tree --filter txt_live_font_change_preserves_raw_anchor_and_complete_source
```
Log: `/private/tmp/unused-bank-fix-focused.log`. 1 passed, 0 failed. Golden 3105 lines; SHA256 `8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454`.

Full reader both command (exit 0):
```
CARGO_NET_OFFLINE=true READER_REG_WORKDIR=/private/tmp/unused-bank-fix-check bash scripts/check-reader-regression.sh both
```
Log: `/private/tmp/unused-bank-fix-both.log`. Head 83 passed; tree 90 passed; 0 failures. Both immutable golden traces remain 3105 lines with SHA256 `8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454`. This change affects identity checks preceding navigation/paging, so one full reader pass was warranted. No full boundary duplication; controller handles final boundary after review. Hardware execution not performed.

## Start/end hashes

| Path | Start SHA256 | End SHA256 |
|---|---|---|
| src/apps/reader/mod.rs | b6ef27f68655778a35183500939ca6adfbf5056a0a350df732fc3c365aa8389a | 97218cd317e73c7f6cef74bbedc7f732843910343f59b75d6e9d2032fdd94367 |
| src/apps/reader/paging.rs | c34bf930c9f82985c7911fa9dccf90e5aeb02deff6c30731bb3f2d5921144c87 | 81e5144087d1a09ed2ba95b6c1fa4d8a38e917c6cb1768d82fc10c0bf169145f |
| src/fonts/cjk.rs | 42f2be4e24026be1de6071ededc6462bc6c770a865e9be646ed4a4fdc8db20dc | cfc5d859dde78efea74d537236dfc1f13ac0cc3ffc1cc811d21ed10da908ec1e |
| host/tests/cjk_unused_bank.rs | 72d094e6b9b2acb6b04d492af20d7236440206aee4321a9bbc2185cb3bc64b25 | 72d094e6b9b2acb6b04d492af20d7236440206aee4321a9bbc2185cb3bc64b25 |
| host/tests/cjk_identity.rs | 42376f2733b6e7051735a008214eba869495428d13b262bb8d64b7b2f6eef1a3 | 42376f2733b6e7051735a008214eba869495428d13b262bb8d64b7b2f6eef1a3 |
| host/tests/cjk_chapter_identity.rs | 5b921d2feb70f788e58f2b6adb412fc78587490d0ce84bba444a6791010f89fb | 5b921d2feb70f788e58f2b6adb412fc78587490d0ce84bba444a6791010f89fb |
| scripts/reader-regression/os/tests/pagination.rs | 454d1a92c96c0749d6ec82bcb70493a898d925e22f5f41424f3d1163168ce06e | 454d1a92c96c0749d6ec82bcb70493a898d925e22f5f41424f3d1163168ce06e |
