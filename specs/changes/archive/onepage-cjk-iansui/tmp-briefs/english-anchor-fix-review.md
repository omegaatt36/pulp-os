# English anchor production repair independent review

## Verdict: BLOCK

Spec verdict: PASS for the human English raw-anchor amendment; FAIL for preserved Latin availability. Quality verdict: BLOCK.

**0 Critical · 1 Important · 0 Minor**

## Important finding

### Unused CJK packs now prevent pure English books from opening

File: `src/apps/reader/paging.rs:74`.

The new unconditional identity establishment calls `current_layout_identity` before selecting a wrapping path. That function validates both body and heading CJK packs. A corrupt installed pack, including a heading pack unused by plain English TXT, therefore returns an error before Latin wrapping and sends the reader into `State::Error`.

Before this patch, supported Latin text collected no fallback metrics and did not establish bank identity; its wrapping did not open these packs. The human amendment authorizes rebuilding English pagination when size changes. It does not require validating unused CJK resources or remove the existing Latin availability contract.

Fix: Track pixel sizes and layout version for all text independently of bank validation. Track and compare bank identities when fallback banks participate in pagination. Preserve bank failure propagation when those resources are needed.

Verified: Traced the scoped snapshot diff, `current_layout_identity` at paging.rs:17–26, `bank_identity`/`open` at cjk.rs:123–158, Latin filtering at cjk.rs:229–234, and `NeedPage` error propagation in mod.rs. No test rerun was needed to establish this direct error path.

## Accepted behavior and evidence

The anchor repair itself is sound: all wrapping paths obtain identity, and explicit invalidation clears the old identity after preserving the raw anchor. The ordinary mismatch path immediately installs the compared identity, preventing a second reset.

The scoped snapshot diff independently matches `english-anchor-fix.diff`: paging.rs only, six additions and three removals. Live SHA256 values match the report: mod.rs `b6ef27f68655778a35183500939ca6adfbf5056a0a350df732fc3c365aa8389a`, paging.rs `c34bf930c9f82985c7911fa9dccf90e5aeb02deff6c30731bb3f2d5921144c87`, frozen pagination.rs `454d1a92c96c0749d6ec82bcb70493a898d925e22f5f41424f3d1163168ce06e`.

Recorded verification was independently inspected: frozen English acceptance GREEN; head 83 and tree 90 tests pass; chapter identity 1, identity 4, bookmarks 15, and EPUB 31 pass. The pre-repair RED log records the adjacent-navigation assertion failure. These suites do not cover a pure English book with an unused corrupt CJK pack. The controller owns the broader cargo gate. Hardware execution was not performed.

Scope: binding English amendment, scoped production diff and report, frozen acceptance test and prior independent test review, surrounding production identity/wrapping/callback code, original patch-start snapshot, and recorded verification logs. No source, tests, fixtures or oracles modified; no test reruns, commits or agents.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 1 | warn |
| MEDIUM | 0 | pass |
| LOW | 0 | pass |

Verdict: BLOCK — resolve the Important availability regression before approval.
