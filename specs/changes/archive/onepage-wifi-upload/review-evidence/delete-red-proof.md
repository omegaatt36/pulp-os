# Delete regression red proof

Expectation source: parent dispatch requirement, dated 2026-10-07. The existing delete contract requires one plain 8.3 filename within the SD root and the complete Content-Length body. HTTP/SD failures must return failure and never success; overlong filename/body must never delete any file. Requirement labels are dispatch context only, not provenance tags.

Tests were written without reading or changing the HTTP implementation. The existing regression harness supplies HTTP requests, a virtual SD card, response/event capture and injected storage failures. Added peer EOF mode allows incomplete bodies to end rather than leave the socket pending.

Command: `scripts/host-test.sh --test upload_regression delete_`

Exit: 101. Actual failure: the overlong-body test observed 200 OK, Deleted(BOOK.TXT), original file removal, and consumed delete injections. All three variants (suffix, long suffix, declared length 14 with early EOF after 13 bytes) reproduced across read slices 1, 7, and whole request. Six other delete tests passed.

```text
    Finished `test` profile [optimized + debuginfo] target(s) in 0.22s
     Running tests/upload_regression.rs (target/aarch64-apple-darwin/debug/build/pulp-host/b522c3570042aceb/out/upload_regression-b522c3570042aceb)

running 7 tests
test delete_rejects_truncated_filename_body ... ok
test delete_storage_failures_never_report_success ... ok
test delete_rejects_overlong_bodies_without_touching_storage ... FAILED
test delete_sandbox_control_removes_a_plain_root_file ... ok
test delete_refuses_names_with_path_separators ... ok
test delete_never_reaches_outside_the_card_root ... ok
test delete_refuses_empty_dot_and_over_long_names ... ok

failures:

---- delete_rejects_overlong_bodies_without_touching_storage stdout ----

thread 'delete_rejects_overlong_bodies_without_touching_storage' (2573361) panicked at host/tests/upload_regression.rs:1259:5:
suffix beyond filename buffer, slice 1, delete injection false: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
suffix beyond filename buffer, slice 1, delete injection false: event Deleted { name: [66, 79, 79, 75, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
suffix beyond filename buffer, slice 1, delete injection false: original file changed or deleted
suffix beyond filename buffer, slice 1, delete injection true: storage delete was called
suffix beyond filename buffer, slice 7, delete injection false: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
suffix beyond filename buffer, slice 7, delete injection false: event Deleted { name: [66, 79, 79, 75, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
suffix beyond filename buffer, slice 7, delete injection false: original file changed or deleted
suffix beyond filename buffer, slice 7, delete injection true: storage delete was called
suffix beyond filename buffer, slice 18446744073709551615, delete injection false: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
suffix beyond filename buffer, slice 18446744073709551615, delete injection false: event Deleted { name: [66, 79, 79, 75, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
suffix beyond filename buffer, slice 18446744073709551615, delete injection false: original file changed or deleted
suffix beyond filename buffer, slice 18446744073709551615, delete injection true: storage delete was called
long suffix, slice 1, delete injection false: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
long suffix, slice 1, delete injection false: event Deleted { name: [66, 79, 79, 75, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
long suffix, slice 1, delete injection false: original file changed or deleted
long suffix, slice 1, delete injection true: storage delete was called
long suffix, slice 7, delete injection false: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
long suffix, slice 7, delete injection false: event Deleted { name: [66, 79, 79, 75, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
long suffix, slice 7, delete injection false: original file changed or deleted
long suffix, slice 7, delete injection true: storage delete was called
long suffix, slice 18446744073709551615, delete injection false: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
long suffix, slice 18446744073709551615, delete injection false: event Deleted { name: [66, 79, 79, 75, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
long suffix, slice 18446744073709551615, delete injection false: original file changed or deleted
long suffix, slice 18446744073709551615, delete injection true: storage delete was called
early peer EOF beyond filename buffer, slice 1, delete injection false: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
early peer EOF beyond filename buffer, slice 1, delete injection false: event Deleted { name: [66, 79, 79, 75, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
early peer EOF beyond filename buffer, slice 1, delete injection false: original file changed or deleted
early peer EOF beyond filename buffer, slice 1, delete injection true: storage delete was called
early peer EOF beyond filename buffer, slice 7, delete injection false: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
early peer EOF beyond filename buffer, slice 7, delete injection false: event Deleted { name: [66, 79, 79, 75, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
early peer EOF beyond filename buffer, slice 7, delete injection false: original file changed or deleted
early peer EOF beyond filename buffer, slice 7, delete injection true: storage delete was called
early peer EOF beyond filename buffer, slice 18446744073709551615, delete injection false: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
early peer EOF beyond filename buffer, slice 18446744073709551615, delete injection false: event Deleted { name: [66, 79, 79, 75, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
early peer EOF beyond filename buffer, slice 18446744073709551615, delete injection false: original file changed or deleted
early peer EOF beyond filename buffer, slice 18446744073709551615, delete injection true: storage delete was called
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    delete_rejects_overlong_bodies_without_touching_storage

test result: FAILED. 6 passed; 1 failed; 0 ignored; 0 measured; 11 filtered out; finished in 0.06s

error: test failed, to rerun pass `-p pulp-host --test upload_regression`

```
