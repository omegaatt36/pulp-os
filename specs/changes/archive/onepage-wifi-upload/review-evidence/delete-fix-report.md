# Delete body fix and hygiene verification

Changed src/apps/upload/http.rs only for behavior: keep the declared Content-Length and reject it when greater than the existing 13-byte filename body buffer. Rejection happens before reads, validation, or storage mutation. Valid bounded requests and truncated-body checks retain their existing paths. No other HTTP behavior changed.

Independent red proof was read from /tmp/wifi-delete-red.md, then independently replayed before editing with scripts/host-test.sh --test upload_regression delete_rejects_overlong_bodies_without_touching_storage. The new regression failed on destructive 200/Deleted responses. Captured at /tmp/wifi-fix-red.log.

Green verification: scripts/host-test.sh --test upload_regression --test upload_http exited 0. upload_http: 24 passed, 0 failed. upload_regression: 18 passed, 0 failed. Output: /tmp/wifi-fix-green.log.

Formatting: rustfmt --edition 2024 ran only on the touched HTTP source and five upload test Rust files.

Shell: bash -n scripts/check-wifi-build.sh exited 0. Actual scripts/check-wifi-build.sh exited 0 with RESULT: wifi build checks ok; output /tmp/wifi-script-final.log. Includes enabled link, optimization, offline boundary and enabled memory report.

Hygiene removed implementation task/spec labels from comments and script display labels in Cargo.toml, scripts/check-wifi-build.sh and host/tests/upload*.rs. Script was regenerated from the original /tmp/wifi-initial-untracked.tar with newline-preserving replacements and atomic installation. Exact line comparison verifies every non-comment executable script line unchanged except intentional hr display labels. A concurrent earlier main build read the script during non-atomic cleanup and failed parsing; the final complete script is verified independently above.

Test weakening gate: no assertions, helpers, expected values or test cases were altered by implementation. Before/after executable-line hash for upload_regression matched exactly before formatting. Formatting original tar test sources in /tmp and comparing executable lines after removing comments/blank lines yields exact matches for upload_http, upload_mdns, upload_connect and upload_session. upload_regression differs from initial tar only through the independently authored new red-proof tests already present before implementation; its executable lines matched the pre-hygiene snapshot. All existing and new HTTP tests pass. No commit or stash.
