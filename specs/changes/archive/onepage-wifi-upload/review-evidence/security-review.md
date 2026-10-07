# Security and guard blast-radius review

Scope: entire staged+unstaged onepage-wifi-upload diff and untracked src/apps/upload/*.rs, host/tests/upload*.rs; requirements R1–R14 read from spec.md. Read-only source review; no repository changes and no credentials printed. Candidate verification compared actual production source with HEAD:src/apps/upload.rs.

## Verified findings

[HIGH / MUST] Reject an overlong delete body before clipping it
File: src/apps/upload/http.rs:156
Issue: Content-Length is capped at 13, and initial_body is newly capped to that length. The capped bytes are then trimmed and passed through is_plain_83. A 14-byte body `BOOK.TXT     X` becomes `BOOK.TXT`; the existing root file is deleted and HTTP 200/Deleted returned even though the submitted name was invalid. Likewise, a declared longer body with only a valid prefix received can delete before the claimed body has arrived. This defeats the new filename guard's intent and the regression tests' explicit contract that overlong names remove nothing.
Verification: line 155 derives max_body = parsed length.min(13); lines 157–158 copy only the first 13 bytes; line 176 trims spaces; line 184 validates the shortened name; line 194 deletes it. HEAD consumed up to 16 initial bytes and rejected the complete 14-byte example for name.len() > 12, so this is a changed behavior rather than an unchanged issue. Existing tests use overlong names whose first 13 bytes also fail 8.3, and therefore miss a valid prefix padded with whitespace.
Fix: parse Content-Length strictly; reject lengths above the allowed filename envelope rather than truncate; read exactly the declared bounded body and validate the actual body before mutation. Add a case with an existing BOOK.TXT, body `BOOK.TXT     X`, whole and one-byte reads, and assert DeleteFailed plus an unchanged card.
Confidence: >95% from concrete source trace; independent executable reproduction delegated by the orchestrator.

## Security sweep conclusions

- Upload sanitize_83 strips the final slash/backslash prefix and restricts output to a short ASCII basename. No traversal reaches root write/append operations.
- New delete is_plain_83 rejects separators, dot components, illegal characters and overlong names when it sees the complete name. Its sole production caller is POST /delete; clipping before validation is the verified exception above.
- mDNS read_name uses get bounds checks, wire name length <=255 and strictly backward compression jumps. Candidate pointer-loop/panic bugs were refuted. Declared questions are all parsed before replying. Additional sections are intentionally ignored by tested contract.
- Oversized UDP packets cannot permanently stop firmware mDNS through RecvError::Truncated: firmware receive storage and parser buffers are both 512; smoltcp::udp::process drops payloads that cannot enqueue into that storage (actual pinned local dependency code inspected). Smaller packets fit recv_from's buffer.
- Upload HTTP boundary-prefix recognition, permissive multipart headers, missing Content-Length enforcement, in-place truncate/append (partial files on disconnect/failure), ignored response write errors and absent auth/CSRF predate the refactor. No new security MUST claimed for those unchanged paths. Spec R8 only requires failure reporting, which the SD error paths do.
- No new hardcoded secret or Wi-Fi password log found. Logs print SSID, filename and fixed error text; radio initialization/connect errors are enum-style driver errors. No config object is logged. Filename raw-name warnings predate the extraction.
- assets/upload.html puts filenames and error/status strings into textContent. Its innerHTML assignments only clear the list with an empty string. XSS candidate refuted.

## Guard/API blast radius

| Changed guard/API | Consumers and risk |
|---|---|
| http::is_plain_83 (new private) | Only serve_request POST /delete; root delete guard blocks direct traversal, but must reject oversize prior to clipping. Both X4 and C61 Wi-Fi firmware share this code. Host upload_http and upload_regression directly exercise it through the production module alias. |
| connect::check_credentials / Limits / within | run_upload_mode pre-screen check, session::run before acquiring radio, host upload_connect and upload_session. Rejects empty/>32-byte SSID and password outside 8..=63, preserving previous WPA2-only policy; bounded association/DHCP applies to both boards. |
| run_upload_mode signature / WIFI acquisition | Only AppManager::run_special_mode calls it. Peripheral steal moves behind successful credential validation and Interface::try_station singleton; no other call site found. |
| session::run / Net resource ownership | Firmware run_upload_mode and host upload_session; borrowed futures are dropped before C and Net fields drop runner/interface before controller. Host tests prove generic ownership, not actual hardware driver re-entry. |
| mdns::handle_packet / serve / Datagrams | Firmware MdnsSocket / serve_mdns; host upload_mdns tests. Parser cannot index attacker bytes unchecked. Receive error intentionally disables mDNS while HTTP continues. |
| board::full_refresh_screen / partial_refresh_screen | Only upload::render_screen uses new board wrapper APIs. C61 partial maps to full refresh; scheduler uses extracted FnStrips separately. No security guard effect. |
| FnStrips moved to public board_c61::epd | C61 scheduler and new board_c61 refresh wrapper. Drawing implementation unchanged; no extra network input reaches draw closures. |
| MemoryBudget::for_build / pool_limit_for / internal_heap_main_bytes | Kernel C61 BUDGET, main_c61 and c61_boot heap constants; memreport and report-c61-memory variant report; board-logic tests. reserve/reserve_in use the selected total pool guard, so Wi-Fi firmware is constrained to 52 KiB+64,000 rather than offline 96 KiB+64,000. Existing MemoryBudget::new and free pool_limit keep offline semantics; no production Wi-Fi caller uses those directly. |
| kernel wifi feature / root forwarding | Root wifi forwards to pulp-kernel/wifi; C61 memory cfg selects budget and actual heap consistently. X4 memory implementation does not consume the C61 guard. |
| host storage write_file / append_root_file aliases | Only production upload_http uses the new seam; both delegate to existing VirtualStorage mutations. Firmware storage APIs are unchanged. |

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 1 | warn |
| MEDIUM | 0 | pass |
| LOW | 0 | pass |

Verdict: WARNING — one HIGH issue should be resolved before merge. Under skill terminology, this verified MUST gates BLOCK.

