# English anchor amendment independent review

Spec verdict: PASS for the frozen test amendment. Quality verdict: WARN for RED evidence provenance accounting. Production acceptance remains RED; no GREEN claim is supported yet.

Scope: `pagination.rs`, frozen amendment diff, binding human all-text rebuild contract, evidence report, and test harness helpers. No tests or production files edited; no cargo runs performed.

The live test SHA256 is `454d1a92c96c0749d6ec82bcb70493a898d925e22f5f41424f3d1163168ce06e`, matching the report and both isolated harness copies. The current git diff matches the frozen amendment. The historical bug-preservation function body is unchanged and retained under `not(feature = "tree")`; the head runner omits `tree`, while the live runner enables it.

The replacement is explicitly authorized by the binding human requirement. The post-change fixed page-3 assertion and unchanged-prefix offset assertion are no longer live-tree acceptance criteria. Their historical head checks remain intact. The replacement adds both real callback paths, a nonzero raw anchor, six-field configured geometry, page containment without a new fixed page number, adjacent navigation consistency, a whole-book forward source walk, and full reverse navigation equality including line bytes and metadata.

The oracle is independently requirement-derived. Expected text comes from a literal source; expected geometry comes from the existing fixture. Monotonic positive page intervals, ordered line spans bounded by each interval, original-byte equality, whitespace-only omitted spans, width and line budget checks together reject omitted or repeated non-whitespace content and stale layouts. The reconstruction equality includes permitted omitted whitespace; it is bookkeeping, not evidence that every whitespace byte is rendered. That matches the explicitly documented wrapping whitespace contract. Scalar-boundary assertions are present, but ASCII input makes them trivial; this amendment supplies English coverage rather than independent multibyte stress coverage.

Harness inspection confirms `Pos` equality compares chapter, page, offset and complete lines; forward/back helpers include both endpoints and have runaway guards. Both saved test logs record the same actual failure at `pagination.rs:256`, `back returns to the same source and layout`, with 0 passed and 1 failed. This is a valid contract failure, not a compile failure. The first resume iteration fails before the quick-cycle iteration or whole-source checks execute; GREEN is therefore still required to exercise those assertions.

## Finding

[MEDIUM] Isolated RED source hash accounting is inaccurate

File: `english-anchor-tests-report.md`, isolated original-production preparation and snapshot hashes.

The report states that the original `cjk.rs` snapshot was copied over the isolated source and lists SHA256 `f6c435df25978fdb83e71e2d0f95cf20600850b607f174df656623dbc22e8935`. The actual RED harness `pulp` symlink targets `/private/tmp/english-anchor-pre-t9`; its `src/fonts/cjk.rs` hashes `42f2be4e24026be1de6071ededc6462bc6c770a865e9be646ed4a4fdc8db20dc`. Its timestamp precedes the RED log, and its difference from the snapshot is an added bank-identity helper. Reader `mod.rs` and `paging.rs` hashes do match the task-start snapshots. The test and runtime RED are credible, but the claimed exact original-source provenance is overstated.

Fix: Record the actual isolated `cjk.rs` hash and compatibility amendment, explaining why unchanged original reader/paging code still demonstrates the pre-identity failure. Alternatively capture RED from an exact original-source export. Do not describe the amended isolated source as byte-identical original production.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 0 | pass |
| MEDIUM | 1 | info |
| LOW | 0 | pass |

Verdict: WARNING — test amendment approved; correct provenance accounting and capture current GREEN before claiming acceptance complete.

## Final evidence re-review

The corrected evidence report now accurately labels the isolated run as original-reader/current-font compatibility RED, discloses the wrong `src/apps/reader/cjk.rs` copy destination, records the unused snapshot hash separately, and records actual `src/fonts/cjk.rs` SHA256 `42f2be4e24026be1de6071ededc6462bc6c770a865e9be646ed4a4fdc8db20dc`. The earlier MEDIUM finding is resolved. The report makes no exact-original-production RED claim.

The actual current-production pre-repair RED remains independently verified from `/private/tmp/english-anchor-green/tree/test.log`: the frozen requirement-derived test fails the adjacent navigation assertion at line 256, with 0 passed and 1 failed. The test hash remains unchanged. Per the controller's clarified evidence requirement, this actual pre-repair current-production RED satisfies the RED discipline; an exact historical replay is not required or authorized.

Final spec verdict: PASS. Final quality/evidence verdict: APPROVE, zero unresolved findings. Current-production GREEN remains pending repair and is required before claiming the implementation satisfies acceptance. No code or tests were modified during this re-review.
