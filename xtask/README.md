# `xtask` (`bamabi-showcase` Automation Runner)

Workspace automation CLI (`cargo xtask <subcommand>`) for `bamabi-showcase`. Operates **standalone** out-of-the-box and automatically expands to cover sibling checkouts (`bamabi-wire`, `bamabi-tools`, `bamabi-core`, `bamabi`) when present on disk or pointed to via `BAM_WIRE_DIR`, `BAM_TOOLS_DIR`, `BAM_CORE_DIR`, `BAM_FACADE_DIR`, or `BAM_INCLUDE_DIR`.

## Subcommands

| Subcommand | Standalone Behavior | Federated Behavior (with Siblings / `BAM_*_DIR`) |
|---|---|---|
| `cargo xtask header [--check] [--test-c99]` | Stages/verifies `target/include/bamabi.h` via `bamabi_sdk_gen` and runs C99/C11 syntax check | Verifies/updates `bamabi-wire/include/bamabi.h` and compiles & runs `bamabi-wire/tests/standalone_c_test.c` |
| `cargo xtask gen-component-headers [--check]` | Stages/verifies `target/include/generated/testbed_components.h` and runs C99 syntax check | Verifies/updates `bamabi-wire/include/generated/testbed_components.h` and runs C99 syntax check |
| `cargo xtask fixtures [--examples] [--benches] [--all]` | Stages all `bamabi-showcase` example/bench Rust, C99, and C++17 guest mods into `target/fixtures/` | Also builds `bamabi-core` asymmetric & C99 test fixtures |
| `cargo xtask lint [--quick]` | Runs `fmt --check`, `clippy --workspace`, `bamabi_host` isolation check, `#![forbid(unsafe_code)]`, and standalone header checks | Also runs `signature-verification` clippy, zero-Bevy `cargo tree`, sibling `forbid(unsafe_code)`, `bevy_lint`, and Bevy idiom audits |
| `cargo xtask ci` | Runs `fmt`, `clippy`, `fixtures`, standalone header checks, and `cargo test --workspace` | Runs the full `ci` pipeline across all present Layer 0–4 workspaces |
| `cargo xtask bench [--permutation] [-- <args>...]` | `--permutation` runs the 34-archetype `permutation_bench` matrix | Without `--permutation`, runs `cargo bench -p bamabi_host` in `bamabi-core` |
| `cargo xtask miri [--all]` | Requires `BAM_WIRE_DIR` (and `BAM_CORE_DIR` for `--all`) | Runs `cargo miri test` (`-Zmiri-tree-borrows`) on Layer 0 / Layer 2 crates |
| `cargo xtask hook <install\|uninstall>` | Installs or removes `.git/hooks/pre-commit` (`cargo xtask lint --quick`) | Same |
| `cargo xtask sign <keygen\|cert\|file\|krl\|verify>` | Pure-Rust Ed25519 keygen, OpenSSH CA certs, `.sig`/`.bam_sig` signing, KRLs, and verification | Same |
