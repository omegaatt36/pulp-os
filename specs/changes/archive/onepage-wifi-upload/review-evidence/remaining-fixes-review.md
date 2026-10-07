# Independent final review of remaining fixes

Verdict: APPROVE. No unresolved MUST or RECALL finding from the reviewed fixes remains. Parent confirmed that production and test edits were finished before this verdict. This review made no production or test changes.

## Requirement resolution and deletion correctness

Read the human clarification under R6 and the proposal's resolved-miss record. They explicitly preserve existing legal FAT short-name deletion while retaining the narrower upload sanitizer. The previous plan conflict is resolved.

Compared current src/apps/upload/http.rs against /tmp/wifi-remaining-start.tar. The only changes are removal of delete's trim, replacement of its sanitizer-equality check, and the new independent is_root_fat_83 helper. Upload parsing, sanitize_83 and is_valid_83_char are unchanged.

The helper requires a 1–8 byte base and optional 1–3 byte extension. It accepts printable ASCII permitted by the pinned embedded-sdmmc parser, including parentheses, apostrophe, percent, at sign, caret, backtick and braces. It rejects FAT-forbidden punctuation, separators, dot-directory names, multiple/misplaced dots, control characters, whitespace and overlong components. Validation receives the exact request body; surrounding whitespace cannot turn into another deletion target. Lowercase remains accepted, with normal case-insensitive behavior supplied by FAT. The preexisting overlong Content-Length and incomplete-body rejection remain before storage access.

The new HTTP tests verify listed punctuation names are actually deleted, only the named target changes, the HTTP response is 200/OK with a flush, and Deleted contains the correct name. Whole-request and one-byte reads exercise framing independence. Separate invalid-name tests preserve the sandbox, including trim-normalization spellings, invalid FAT punctuation, lengths and path attempts. Upload compatibility independently verifies A(B).TXT produces AB.TXT with exact bytes and preserves OTHER.TXT.

## Test preservation and evidence

Mechanical comparison with /tmp/wifi-remaining-start.tar confirmed all regression source before the new FAT-oracle tests is byte-for-byte identical. All seven existing wifi_budget test bodies are also byte-for-byte identical; imports and two new behavioral tests were added.

The initial FAT test draft tried punctuation in unsupported extensions and expected GET /files to list them. The author corrected those new cases to TXT extensions, consistent with the existing supported-extension filter, and added the literal A(B).TXT case. This corrects the new oracle rather than weakening an existing acceptance test. Predicate source still validates base and extension with the same character rule.

Read fat-name-red-proof.md, remaining-test-report.md and fat-name-fix-report.md. Red output shows actual rejected legitimate filenames and destructive trim behavior before the fix. Independently read /tmp/wifi-fat-green.log: upload_http 24 passed and upload_regression 21 passed, zero failures/ignored. This reviewer did not repeat the same test run. Final aggregate host/build execution remains the parent's verification responsibility.

## Aggregate budget coverage

Read the new tests and real MemoryBudget::reserve_in/release implementation. The tests construct both build variants and independently assert 117248-byte Wi-Fi and 162304-byte offline capacities. They fill the real internal pool across multiple classes, choose a class with remaining class capacity, and assert an additional reservation fails specifically as PoolExhausted. This distinguishes aggregate enforcement from per-class limits. Pool and every class's accounting remain unchanged after refusal. Release, exact refill, and release-to-zero are exercised.

The same sequence runs under uninitialized, degraded and ready PSRAM states. A separate offline positive control successfully reserves above the Wi-Fi ceiling, catching an accidentally universal smaller budget. These are accounting-policy tests, not a claim to measure allocator availability or physical heap high-water behavior.

Read wifi-budget-mutation-report.md and both raw mutation logs. The wrong-build-variant mutant fails with 162304 actual versus 117248 expected. The missing-enforcement mutant passes the initial capacity assertion but fails the real beyond-pool reservation assertion. Both logs report exactly the new aggregate test failing (8 passed, 1 failed), confirming both wiring and actual rejection are tested. Production memory code was not changed for this coverage fix.

## Known limits retained

Inherited G5 SD close-error swallowing remains explicitly deferred; host VirtualStorage does not validate physical FAT metadata-flush failure. Association, DHCP, physical multicast reception, actual radio teardown/re-entry behavior, internal heap/stack high-water marks and current draw remain hardware-unverified. No new claim of hardware acceptance is made by this approval.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 0 | pass |
| MEDIUM | 0 | info |
| LOW | 0 | note |

Verdict: APPROVE — reviewed prior MUST/RECALL items are resolved; documented inherited and hardware limits remain.
