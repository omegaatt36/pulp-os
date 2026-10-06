# Unused fallback bank repair independent review

## Verdict: APPROVE

Spec verdict: PASS. Quality verdict: APPROVE.

**0 Critical · 0 Important · 0 Minor production findings**

No production findings. The Important finding in `english-anchor-fix-review.md` is resolved by this follow-up patch; that earlier BLOCK documents the superseded source state.

## Independent source assessment

`LayoutIdentity` now records body/heading pixel sizes and layout version independently of optional bank identities (`mod.rs:332`). Initial `current_layout_identity` performs no bank lookup when no bank previously participated (`paging.rs:17`). This preserves all-text size/version comparison without creating a CJK resource dependency for Latin-only wrapping.

After fallback metrics staging, `record_fallback_identity` records only banks represented by staged metric entries (`paging.rs:42`, `cjk.rs:409`). Supported Latin produces no such entries. Participating bank identities remain in the reader identity when later windows contain only Latin and when suspension clears transient CJK caches; identity checking therefore still detects same-size replacements of previously used banks. Real fallback staging and identity comparison continue to propagate corrupt-pack errors.

Raw-anchor behavior remains intact. Explicit invalidation captures the anchor before resetting pagination and clears identity, avoiding the prior double reset. Ordinary identity mismatch installs the compared identity immediately after invalidation. The existing NeedPage restore loop rebuilds from the beginning and chooses the interval containing the prior raw offset. The NeedIndex chapter-specific anchor handling is unchanged.

The new bank-use query is bounded by the existing staged metric collection and introduces no allocation or I/O. Body and heading pixel sizes are distinct for every configured size, so querying by pixel size identifies their roles unambiguously.

## Evidence inspected

Read the scoped diff, production report, frozen unused-bank tests, frozen English test, source identity/staging/callback/restore paths, and actual recorded RED/GREEN logs. No test reruns were performed; the controller owns the broader cargo gate.

`/private/tmp/unused-bank-red.log` contains three real pre-repair failures: corrupt unused body, corrupt unused heading, and valid unused bank reads. `/private/tmp/unused-bank-fix-host.log` contains all three GREEN results and identity/chapter coverage GREEN. Finalized log and report agree on 75 integration passes across ten binaries, including a separate three-test `cjk_surface_fixes` binary. The earlier review count note was mistaken: the first one-test result belongs to chapter identity, not harness units. Re-reading each binary header resolved the note; no evidence finding remains.

`/private/tmp/unused-bank-fix-focused.log` records the frozen English raw-anchor/navigation/source-conservation acceptance GREEN. `/private/tmp/unused-bank-fix-both.log` records full reader regression GREEN: head 83, tree 90; both golden traces retain 3105 lines and SHA256 `8cb6e31a70a32cdc05d30fab45ee0c7983da8cde6a55ba8bb45a0540f8d3f454`.

Live hashes independently match the repair report: mod.rs `97218cd317e73c7f6cef74bbedc7f732843910343f59b75d6e9d2032fdd94367`, paging.rs `81e5144087d1a09ed2ba95b6c1fa4d8a38e917c6cb1768d82fc10c0bf169145f`, cjk.rs `cfc5d859dde78efea74d537236dfc1f13ac0cc3ffc1cc811d21ed10da908ec1e`. Frozen unused-bank test SHA256 remains `72d094e6b9b2acb6b04d492af20d7236440206aee4321a9bbc2185cb3bc64b25`; frozen English pagination test remains `454d1a92c96c0749d6ec82bcb70493a898d925e22f5f41424f3d1163168ce06e`.

Source, tests, fixtures and oracles remained read-only during this review. Only this report was written. Hardware execution was not performed.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 0 | pass |
| MEDIUM | 0 | pass |
| LOW | 0 | pass |

Verdict: APPROVE — no unresolved production findings in the reviewed follow-up patch.
