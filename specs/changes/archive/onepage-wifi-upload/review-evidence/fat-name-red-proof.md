# FAT filename deletion red proof

Expectation sources: user-authorized remaining review fixes and pinned embedded-sdmmc ShortFileName::create_from_str, Cargo.lock revision 0bf12548d1144b0e2b06b290acde3e4bb46cd91b. The parser source permits parentheses, apostrophe, percent, at sign, caret, backtick and braces in 8.3 names. SD-root containment remains mandatory. GET/files must return the seeded name, and POST/delete must delete that listed filename with 200/Deleted while preserving OTHER.TXT. Expectations were not derived from production HTTP code or output.

Command: `scripts/host-test.sh --test upload_regression listed_fat_short_names_with_punctuation_can_be_deleted`

Pre-fix exit: 101. The test collected failure status/events and surviving target files across whole and one-byte reads. Actual output follows.

```text
   Compiling pulp-host v0.1.0 (/Users/raiven_kao/dev/pulp-os/host)
    Finished `test` profile [optimized + debuginfo] target(s) in 1.60s
     Running tests/upload_regression.rs (target/aarch64-apple-darwin/debug/build/pulp-host/b522c3570042aceb/out/upload_regression-b522c3570042aceb)

running 1 test
test listed_fat_short_names_with_punctuation_can_be_deleted ... FAILED

failures:

---- listed_fat_short_names_with_punctuation_can_be_deleted stdout ----

thread 'listed_fat_short_names_with_punctuation_can_be_deleted' (2596472) panicked at host/tests/upload_regression.rs:1369:5:
FAT name "A(.TXT", slice 1: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A(.TXT", slice 1: event DeleteFailed
FAT name "A(.TXT", slice 1: target still exists
FAT name "A(.TXT", slice 18446744073709551615: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A(.TXT", slice 18446744073709551615: event DeleteFailed
FAT name "A(.TXT", slice 18446744073709551615: target still exists
FAT name "A).TXT", slice 1: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A).TXT", slice 1: event DeleteFailed
FAT name "A).TXT", slice 1: target still exists
FAT name "A).TXT", slice 18446744073709551615: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A).TXT", slice 18446744073709551615: event DeleteFailed
FAT name "A).TXT", slice 18446744073709551615: target still exists
FAT name "A'.TXT", slice 1: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A'.TXT", slice 1: event DeleteFailed
FAT name "A'.TXT", slice 1: target still exists
FAT name "A'.TXT", slice 18446744073709551615: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A'.TXT", slice 18446744073709551615: event DeleteFailed
FAT name "A'.TXT", slice 18446744073709551615: target still exists
FAT name "A%.TXT", slice 1: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A%.TXT", slice 1: event DeleteFailed
FAT name "A%.TXT", slice 1: target still exists
FAT name "A%.TXT", slice 18446744073709551615: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A%.TXT", slice 18446744073709551615: event DeleteFailed
FAT name "A%.TXT", slice 18446744073709551615: target still exists
FAT name "A@.TXT", slice 1: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A@.TXT", slice 1: event DeleteFailed
FAT name "A@.TXT", slice 1: target still exists
FAT name "A@.TXT", slice 18446744073709551615: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A@.TXT", slice 18446744073709551615: event DeleteFailed
FAT name "A@.TXT", slice 18446744073709551615: target still exists
FAT name "A^.TXT", slice 1: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A^.TXT", slice 1: event DeleteFailed
FAT name "A^.TXT", slice 1: target still exists
FAT name "A^.TXT", slice 18446744073709551615: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A^.TXT", slice 18446744073709551615: event DeleteFailed
FAT name "A^.TXT", slice 18446744073709551615: target still exists
FAT name "A`.TXT", slice 1: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A`.TXT", slice 1: event DeleteFailed
FAT name "A`.TXT", slice 1: target still exists
FAT name "A`.TXT", slice 18446744073709551615: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A`.TXT", slice 18446744073709551615: event DeleteFailed
FAT name "A`.TXT", slice 18446744073709551615: target still exists
FAT name "A{.TXT", slice 1: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A{.TXT", slice 1: event DeleteFailed
FAT name "A{.TXT", slice 1: target still exists
FAT name "A{.TXT", slice 18446744073709551615: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A{.TXT", slice 18446744073709551615: event DeleteFailed
FAT name "A{.TXT", slice 18446744073709551615: target still exists
FAT name "A}.TXT", slice 1: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A}.TXT", slice 1: event DeleteFailed
FAT name "A}.TXT", slice 1: target still exists
FAT name "A}.TXT", slice 18446744073709551615: expected 200 OK, got "HTTP/1.0 500 Internal Server Error\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nInvalid filename"
FAT name "A}.TXT", slice 18446744073709551615: event DeleteFailed
FAT name "A}.TXT", slice 18446744073709551615: target still exists
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    listed_fat_short_names_with_punctuation_can_be_deleted

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 19 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p pulp-host --test upload_regression`

```

## Exact filename whitespace/control red proof

Expected source: exact complete FAT filename requirement plus the pinned parser's InvalidCharacter rejection for space and U+0000 through U+001F. Leading/trailing spaces, tabs, and newline around KEEP.TXT must return failure/DeleteFailed without mutating any sandbox file. This prohibits trimming into an existing filename.

Command: `scripts/host-test.sh --test upload_regression delete_rejects_invalid_fat_short_names_without_normalizing`

Pre-fix exit: 101. Actual output follows.

```text
   Compiling pulp-host v0.1.0 (/Users/raiven_kao/dev/pulp-os/host)
    Finished `test` profile [optimized + debuginfo] target(s) in 2.35s
     Running tests/upload_regression.rs (target/aarch64-apple-darwin/debug/build/pulp-host/b522c3570042aceb/out/upload_regression-b522c3570042aceb)

running 1 test
test delete_rejects_invalid_fat_short_names_without_normalizing ... FAILED

failures:

---- delete_rejects_invalid_fat_short_names_without_normalizing stdout ----

thread 'delete_rejects_invalid_fat_short_names_without_normalizing' (2602689) panicked at host/tests/upload_regression.rs:1003:5:
36 problems:
delete  KEEP.TXT (" KEEP.TXT") slice 1: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
delete  KEEP.TXT (" KEEP.TXT") slice 1: event Deleted { name: [75, 69, 69, 80, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
delete  KEEP.TXT (" KEEP.TXT") slice 1: sandbox changed, removed ["d1/d2/d3/d4/d5/sd/KEEP.TXT"], added []
delete  KEEP.TXT (" KEEP.TXT") slice 18446744073709551615: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
delete  KEEP.TXT (" KEEP.TXT") slice 18446744073709551615: event Deleted { name: [75, 69, 69, 80, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
delete  KEEP.TXT (" KEEP.TXT") slice 18446744073709551615: sandbox changed, removed ["d1/d2/d3/d4/d5/sd/KEEP.TXT"], added []
delete KEEP.TXT  ("KEEP.TXT ") slice 1: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
delete KEEP.TXT  ("KEEP.TXT ") slice 1: event Deleted { name: [75, 69, 69, 80, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
delete KEEP.TXT  ("KEEP.TXT ") slice 1: sandbox changed, removed ["d1/d2/d3/d4/d5/sd/KEEP.TXT"], added []
delete KEEP.TXT  ("KEEP.TXT ") slice 18446744073709551615: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
delete KEEP.TXT  ("KEEP.TXT ") slice 18446744073709551615: event Deleted { name: [75, 69, 69, 80, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
delete KEEP.TXT  ("KEEP.TXT ") slice 18446744073709551615: sandbox changed, removed ["d1/d2/d3/d4/d5/sd/KEEP.TXT"], added []
delete 	KEEP.TXT ("\tKEEP.TXT") slice 1: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
delete 	KEEP.TXT ("\tKEEP.TXT") slice 1: event Deleted { name: [75, 69, 69, 80, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
delete 	KEEP.TXT ("\tKEEP.TXT") slice 1: sandbox changed, removed ["d1/d2/d3/d4/d5/sd/KEEP.TXT"], added []
delete 	KEEP.TXT ("\tKEEP.TXT") slice 18446744073709551615: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
delete 	KEEP.TXT ("\tKEEP.TXT") slice 18446744073709551615: event Deleted { name: [75, 69, 69, 80, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
delete 	KEEP.TXT ("\tKEEP.TXT") slice 18446744073709551615: sandbox changed, removed ["d1/d2/d3/d4/d5/sd/KEEP.TXT"], added []
delete KEEP.TXT	 ("KEEP.TXT\t") slice 1: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
delete KEEP.TXT	 ("KEEP.TXT\t") slice 1: event Deleted { name: [75, 69, 69, 80, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
delete KEEP.TXT	 ("KEEP.TXT\t") slice 1: sandbox changed, removed ["d1/d2/d3/d4/d5/sd/KEEP.TXT"], added []
delete KEEP.TXT	 ("KEEP.TXT\t") slice 18446744073709551615: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
delete KEEP.TXT	 ("KEEP.TXT\t") slice 18446744073709551615: event Deleted { name: [75, 69, 69, 80, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
delete KEEP.TXT	 ("KEEP.TXT\t") slice 18446744073709551615: sandbox changed, removed ["d1/d2/d3/d4/d5/sd/KEEP.TXT"], added []
delete 
KEEP.TXT ("\nKEEP.TXT") slice 1: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
delete 
KEEP.TXT ("\nKEEP.TXT") slice 1: event Deleted { name: [75, 69, 69, 80, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
delete 
KEEP.TXT ("\nKEEP.TXT") slice 1: sandbox changed, removed ["d1/d2/d3/d4/d5/sd/KEEP.TXT"], added []
delete 
KEEP.TXT ("\nKEEP.TXT") slice 18446744073709551615: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
delete 
KEEP.TXT ("\nKEEP.TXT") slice 18446744073709551615: event Deleted { name: [75, 69, 69, 80, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
delete 
KEEP.TXT ("\nKEEP.TXT") slice 18446744073709551615: sandbox changed, removed ["d1/d2/d3/d4/d5/sd/KEEP.TXT"], added []
delete KEEP.TXT
 ("KEEP.TXT\n") slice 1: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
delete KEEP.TXT
 ("KEEP.TXT\n") slice 1: event Deleted { name: [75, 69, 69, 80, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
delete KEEP.TXT
 ("KEEP.TXT\n") slice 1: sandbox changed, removed ["d1/d2/d3/d4/d5/sd/KEEP.TXT"], added []
delete KEEP.TXT
 ("KEEP.TXT\n") slice 18446744073709551615: failure response contains "200 OK": "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nOK"
delete KEEP.TXT
 ("KEEP.TXT\n") slice 18446744073709551615: event Deleted { name: [75, 69, 69, 80, 46, 84, 88, 84, 0, 0, 0, 0, 0], name_len: 8 }
delete KEEP.TXT
 ("KEEP.TXT\n") slice 18446744073709551615: sandbox changed, removed ["d1/d2/d3/d4/d5/sd/KEEP.TXT"], added []
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    delete_rejects_invalid_fat_short_names_without_normalizing

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 20 filtered out; finished in 0.21s

error: test failed, to rerun pass `-p pulp-host --test upload_regression`

```
