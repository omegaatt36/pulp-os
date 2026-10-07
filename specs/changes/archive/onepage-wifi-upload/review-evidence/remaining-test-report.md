# Remaining review test report

## Expected behavior and isolation

Test author changed only host/tests/upload_regression.rs and board-logic/tests/wifi_budget.rs, plus review evidence. No production implementation was read or changed. Expectations come from user-authorized review fixes, the updated upload compatibility requirement A(B).TXT -> AB.TXT, pinned embedded-sdmmc ShortFileName::create_from_str at Cargo.lock revision 0bf12548d1144b0e2b06b290acde3e4bb46cd91b, and the documented build budget (Wi-Fi 52*1024+64000; offline 96*1024+64000).

## HTTP regression coverage

GET/files must list each seeded punctuation filename; POST/delete must accept that listed name and remove only that file. Cases cover parentheses (including A(B).TXT), apostrophe, %, @, ^, backtick and braces, using whole and one-byte reads. Separate rejection cases preserve all sandbox files for forbidden FAT characters, oversized base/extension, misplaced dots, slash/backslash paths and root escapes. Leading/trailing spaces, tabs, and newline around KEEP.TXT also exercise exact-name handling without trimming. Existing overlong-body, split-read, incomplete-body, storage-failure and root-containment tests remain intact.

Upload compatibility is checked independently: uploading A(B).TXT creates AB.TXT and reports that sanitized name, preserving OTHER.TXT and never creating the original spelling.

Actual pre-fix red proof is in fat-name-red-proof.md. No failure was manufactured. Host virtual listing omits unsupported extensions such as T(; cases therefore use the supported TXT extension and punctuation in the base, exercising names the browser actually receives.

Commands and results before the remaining HTTP implementation fix:

- `scripts/host-test.sh --test upload_regression listed_fat_short_names_with_punctuation_can_be_deleted`: exit 101, one failing regression, matching the deletion acceptance defect.
- `scripts/host-test.sh --test upload_regression delete_`: exit 0, 8 passed before adding whitespace/control cases.
- `scripts/host-test.sh --test upload_regression delete_rejects_invalid_fat_short_names_without_normalizing`: after adding whitespace/control cases, pre-fix exit 101; observed 200/Deleted and sandbox loss. Actual red output is appended to fat-name-red-proof.md.
- `scripts/host-test.sh --test upload_regression upload_keeps_existing_parenthesis_sanitization`: exit 0, 1 passed.

## Memory budget coverage

The new behavioral test constructs MemoryBudget::for_build(true/false), asserts independently specified capacities, reserves an exact aggregate fit spanning multiple classes, rejects an additional 16-byte reservation as PoolExhausted, and confirms rejection changes neither pool nor class accounting. It releases a reservation, refills to the same boundary, then releases all allocations to zero. The same behavior is exercised with NotInitialised, Degraded(NotDetected), and Ready(2 MiB) PSRAM states. A separate offline positive control reserves 16 bytes above the Wi-Fi ceiling across multiple classes.

This is coverage-only: the current memory implementation was already correct, so the tests passed immediately. There is no memory red proof; the orchestrator will validate the new boundary test against an isolated bad-wiring mutant.

- `scripts/test-board-logic.sh --test wifi_budget`: exit 0, 9 passed.
- `scripts/test-board-logic.sh`: exit 0, 321 unit tests + 3 font-memory tests + 9 Wi-Fi budget tests passed; zero doc tests.

The orchestrator must append final post-fix HTTP verification and isolated mutation results after implementation.
