# Final broad review — onepage-wifi-upload

Scope: tracked diff against 8702340, upload production modules, corresponding host test coverage, spec/proposal/progress, firmware manager/scheduler/board paths, pinned esp-radio and embassy-net teardown, and real embedded-sdmmc persistence semantics. Read-only source review; old test suites were not rerun and no production files were changed.

## Verified finding: existing valid FAT filenames cannot be deleted

[HIGH / MUST candidate, requirement conflict] Preserve deletion of supported existing FAT names.

File: src/apps/upload/http.rs:184, src/apps/upload/http.rs:445, src/apps/upload/http.rs:450

Trigger: place A(B).TXT on the SD card using a computer, enter Upload, and press its Delete button in the browser. The file is listed but POST /delete now returns 500 Invalid filename. Before this change the same plain 8.3 name passed the nonempty/12-byte check and reached storage::delete_file.

Verification:
- The pinned real SD dependency at /Users/raiven_kao/.cargo/git/checkouts/embedded-sdmmc-rs-cc584bb199b7ebf5/0bf1254/src/filesystem/filename.rs:90-165 accepts parentheses (and apostrophe, percent, at sign, caret, backtick, braces) in ShortFileName::create_from_str. They are absent from its explicit invalid-character list.
- kernel/src/drivers/storage.rs:262 lists root short names; kernel/src/drivers/dir_entry.rs:101 only filters leading dot/underscore and unsupported extensions. A(B).TXT therefore remains visible.
- assets/upload.html:203 sends the displayed name through the Delete button.
- New is_plain_83 requires equality with sanitize_83 output. Its whitelist at http.rs:450 drops both parentheses, transforming A(B).TXT to AB.TXT and rejecting the original. This is not a path-traversal case.
- Baseline upload.rs /delete only rejected empty or >12-byte names, so this is introduced by T6 hardening, unlike upload renaming itself.

Requirement: R6 says preserve existing HTTP upload/list/delete behavior. T3 contract says plain filename without restricting this character set. However, T6 implementation instructions require sharing the upload/delete validator, while the established upload sanitizer deliberately has a narrower character set. This is a requirement/plan conflict. Ask the owner to resolve it before implementing a broader validator; do not silently change upload naming behavior. Suggested resolution: retain upload sanitation and independently validate deletions as exact, root-contained FAT 8.3 names, with tests for A(B).TXT plus separator rejection.

## Independently observed, assigned to security sweep

http.rs:158 clips Content-Length to 13 and line 177 trims before validation. A longer malformed request whose first 13 bytes trim to a valid filename can delete that file without reading the declared remaining body. Parent reports this independently reproduced; avoid duplicate findings in final synthesis.

## Inherited/deferred limitation, not a new diff finding

kernel/src/drivers/storage.rs:119 and :144 discard close_file errors. The pinned async_volume_mgr.rs:1087 explicitly defers directory entry updates until close; close_file at :1098 invokes flush_file, which writes FAT info and the directory entry at :1114-1125. A card error during that write can therefore leave an old/zero file size while HTTP returns Uploaded/200. The host stand-in at host/src/drivers/storage.rs:34-39 forwards to VirtualStorage and does not exercise the real SD close path. This limits R7/R8 evidence, but is unchanged behavior and explicitly tracked/deferred as G5 in progress. Preserve that user decision; do not invent a new regression or claim complete SD failure coverage.

Other inherited observations excluded: multipart boundary prefix recognition without trailer validation, errors ignored while writing successful HTTP responses, unsupported upload filename sanitation collisions. These were present in the baseline upload.rs.

## Lifecycle/cancellation verification

- manager.rs:803 calls run_upload_mode only from the isolated special-mode scheduler path.
- session::run owns C, stage futures borrow it, and select drops stage futures before C at return. Back is first in select, so ready Back wins before starting another async stage.
- Net fields drop runner/interface before controller. Pinned esp-radio Interface::drop at wifi/mod.rs:1633 releases singleton bits; WifiController owns WifiRefGuard, whose drop at :2606 decrements references and calls wifi_deinit on the last guard. No other ESP-NOW/sniffer guard is created here.
- The failed WifiController::new path drops the previously acquired local Interface. Acquisition uses try_station rather than panicking.
- DHCP and serving select scopes drop borrowed runner.run futures before Net. TCP and UDP socket Drop implementations remove their stack socket handles (embassy-net tcp.rs:466, udp.rs:388).
- mDNS serves in a persistent independent future alongside HTTP and runner. Its queries no longer cancel an in-flight HTTP request. An mDNS receive failure parks only that service; HTTP remains alive.
- C61 rendering uses the blocking full_refresh adapter, so there is no C61 mid-refresh async cancellation seam. Actual radio teardown completion, repeated-entry heap behavior and physical multicast reception remain hardware-unverified as documented.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 1 | warn |
| MEDIUM | 0 | info |
| LOW | 0 | note |

Verdict: WARNING — one verified deletion regression needs a requirement/plan resolution; security findings are reported separately. No additional confirmed new lifecycle or mDNS cross-task bug.
