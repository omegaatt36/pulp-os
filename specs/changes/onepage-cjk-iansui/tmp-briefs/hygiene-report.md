Comment hygiene verification

Scope: comment-only edits in four owned Rust test files. No Cargo commands or mutants were run.

Method: files were saved before editing in `/private/tmp/cjk-hygiene-start`. Each saved/current file was compared after stripping line and block comments, then tokenized. The comparison confirms executable tokens are identical.

Results:
- `fontconv/tests/roundtrip.rs`: comment-stripped tokens identical: **True**; SHA-256 before `dc9623afd870c326e055af192c7128e298d0958f765e24f02d9a89bfe022d9ca`, after `ed741c853cd1c8533d5dca83517b1bbd85d0c0eae9b504cc5e7c04c509e4e7ae`.
- `fontpack/tests/missing_glyph.rs`: comment-stripped tokens identical: **True**; SHA-256 before `d13a901e6c35a6aedbc83a7083d78429c34d6c49d564f99a047e7625d83824ae`, after `38b87f35c5c67e20ea55336caa48c1e9daff355ff2d21af465743a4531553b3e`.
- `fontpack/tests/reader.rs`: comment-stripped tokens identical: **True**; SHA-256 before `d31c01340e913ace54129d555582e98f9d16b790b6244d4f85b825bf40a6366c`, after `a99f18bcd9a25f55bd36411b9a14f6be8ce52c71193bdb6957248061b0a70709`.
- `fontpack/tests/reader_faults.rs`: comment-stripped tokens identical: **True**; SHA-256 before `b16d5c0e4e4a54919182280a4b51176bd641961205d54271747d9596681c269e`, after `57ca2cb27e887fe8af98496394be2909ef18fb057fbec288e1b695b1da34bd3c`.

`git diff --check` passed. Change-local R2/R3/R4/R5/T1/T2 and `t3-contract.md` references were removed from comments. Hex byte values `0xA0` and Unicode code points were preserved. SHA changes are disclosed as comment hygiene only; no behavior was weakened.
