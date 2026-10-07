# Wi-Fi budget mutation verification

Coverage-only change: real implementation was already correct and new tests passed before any production change. No artificial production red-proof is claimed.

Tests were copied into an isolated `/tmp/wifi-budget-mutant-3ygyadrh/board-logic` crate. Repository production source remained untouched.

Command for each mutation: `cargo test --manifest-path /tmp/wifi-budget-mutant-3ygyadrh/board-logic/Cargo.toml --target aarch64-apple-darwin --config 'unstable.build-std=["std","test"]' --test wifi_budget`.

1. Change `pool_limit_for(self.wifi, self.status, region)` to `pool_limit_for(false, self.status, region)`: exit101, 8 passed/1 failed. Aggregate boundary test reports 162304 actual versus 117248 expected for Wi-Fi. Raw output: `wifi-budget-wrong-variant-mutation.log`.
2. Restore original source, replace reservation's `let pool = self.pool_limit(region);` with `let pool = usize::MAX;`: exit101, 8 passed/1 failed. The actual reserve call succeeds beyond the capacity and fails `reject beyond aggregate pool`. Raw output: `wifi-budget-missing-enforcement-mutation.log`.

Expected capacities are literal contract values (52KiB+64000 for Wi-Fi, 96KiB+64000 offline). Both interface wiring and actual aggregate rejection are independently exercised.
