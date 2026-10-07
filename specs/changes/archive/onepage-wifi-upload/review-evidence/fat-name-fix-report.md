# FAT short-name deletion fix

Production ownership: src/apps/upload/http.rs only. This change replaces delete's upload-sanitizer equality guard with an independent exact root FAT 8.3 validator. It accepts the printable ASCII character set accepted by pinned embedded-sdmmc, including parentheses, apostrophe, percent, at sign, caret, backtick and braces. Base length is 1–8; an optional extension has length 1–3. Path separators, extra/misplaced dots, invalid FAT punctuation, whitespace, ASCII control bytes and non-ASCII bytes are refused before storage mutation. The earlier declared-body-length rejection and truncated-body checks remain.

The pinned parser was read directly from the local embedded-sdmmc checkout, revision recorded by the independent test author. HTTP uses a root basename contract, so special dot-directory names and normalization spellings remain forbidden even where the raw FAT parser has special handling.

Removed delete's trim operation: validation now sees the exact complete filename body. The exact-name policy was confirmed with the orchestrator before applying this final change. Leading/trailing spaces, tabs and newlines must not delete the trimmed target; independent whitespace red proof exists in fat-name-red-proof.md. Upload sanitize and its original character whitelist were left unchanged; uploading A(B).TXT still saves AB.TXT.

Red evidence: read the independent fat-name-red-proof.md and replayed scripts/host-test.sh --test upload_regression listed_fat_short_names_with_punctuation_can_be_deleted before applying the change. Exit 101, actual failure captured in /tmp/wifi-fat-own-red.log. The author separately captured whitespace failure before the final production change.

Green command: scripts/host-test.sh --test upload_http --test upload_regression
Exit 0. Actual summary:

    upload_http:       24 passed; 0 failed; 0 ignored
    upload_regression: 21 passed; 0 failed; 0 ignored

Full command output: /tmp/wifi-fat-green.log. This includes listed FAT punctuation deletion, whitespace/invalid FAT spelling rejection, unchanged upload parenthesis sanitization, overlong/truncated delete bodies, storage failure, path safety and byte-exact uploads.

Formatting: rustfmt --edition 2024 src/apps/upload/http.rs, followed by rustfmt --edition 2024 --check src/apps/upload/http.rs; exit 0.

Test weakening gate: after the independent author finalized tests, SHA-256 snapshots were recorded for all five host/tests/upload*.rs files. After implementation and test execution, all five files matched those snapshots byte-for-byte. No assertions, helpers, expected values, ignores or timeouts changed. Tests remained strictly read-only throughout this production task. No memory/budget code changed, no commit or stash.
