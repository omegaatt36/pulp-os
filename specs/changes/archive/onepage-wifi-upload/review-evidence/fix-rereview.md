# Re-review: deletion length fix and review hygiene

Verdict for this incremental change: APPROVE. No new verified issue. The earlier valid-FAT-name requirement/plan conflict remains unresolved and is outside this approval.

## Production fix

Compared src/apps/upload/http.rs with the initial untracked tar snapshot. The only functional change is at lines 158-163: preserve declared Content-Length and reject lengths greater than the 13-byte body buffer before copying, reading, trimming or invoking storage. This prevents an overlong request from being truncated to an otherwise valid deletion target. Existing complete bounded-body handling and short-body rejection remain intact. Saturating Content-Length parsing also drives an overflowed numeric length into this refusal branch.

## Tests and red-tier boundary

Reviewed all three added tests in host/tests/upload_regression.rs:
- Overlong suffixes and declared lengths beyond available bytes: sliced reads (1/7/whole), unchanged original file, DeleteFailed, failure HTTP response, and a pending delete injection proving storage was not called.
- Missing SD or injected delete failure: no success event/response and unchanged original file; the injection must be consumed in the injected-card-error case.
- Truncated ordinary filename: EOF before declared length, failure response/event, unchanged matching shorter file, and unconsumed delete injection.

The peer_eof harness option defaults to false. Existing scenarios retain their previous pending-read-on-exhaustion behavior. New EOF scenarios return Ok(0) only once all request bytes have been read. This models read-half-close while still allowing the server to write an HTTP response.

Mechanical verification: read originals directly from /tmp/wifi-initial-untracked.tar, ran each old/current test source through rustfmt --edition 2024 --emit stdout, removed blank/full-line-comment lines, and compared. upload_connect.rs, upload_http.rs, upload_mdns.rs and upload_session.rs are identical under that normalization. upload_regression.rs differs only by the peer_eof fields/default/method/read branch/construction plumbing and the three appended tests. No existing executable assertion was removed or weakened. Formatting changes are not executable test changes; comments/formatting were not mistaken for altered acceptance criteria.

No old suites were rerun by this reviewer. Test execution evidence remains with the parent/test author; this is an independent source review.

## Script/config hygiene

At review time scripts/check-wifi-build.sh contains the restored conditional. `bash -n scripts/check-wifi-build.sh` passed with exit 0. Compared original and current script after removing comments, blank lines, and display-only `hr` calls: all remaining executable lines are identical. Versions, feature checks, radio optimization requirements, enabled/offline build invocations, ELF probes and memory-budget checks are preserved. Changes to hr strings only remove task/requirement labels.

Cargo.toml changes associated with hygiene are comments; reviewed feature/profile/dependency changes remain the already-reviewed implementation. No behavior change arose from removing task labels.

This verification describes the filesystem snapshot observed during this re-review; later edits need their usual checks. The transient deleted `if` is no longer present and is not an outstanding finding.

## Remaining decision

The A(B).TXT deletion regression remains pending user resolution: R6 preserves existing deletion behavior, whereas the T6 brief mandates sharing the narrower upload validator. The length fix does not change that policy. G5 storage close-error swallowing remains inherited and explicitly deferred, not a newly introduced finding.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 0 | pass |
| MEDIUM | 0 | info |
| LOW | 0 | note |

Verdict: APPROVE incremental length fix and hygiene; overall feature approval still depends on the separately recorded FAT-name decision.
