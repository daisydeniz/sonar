//! `uniffi-bindgen` CLI pinned to the workspace's UniFFI version.
//!
//! Used by `scripts/build-core.sh` to generate Swift bindings in library
//! mode (`generate --library <libsonar_ffi.a> --language swift`). Building it
//! from the workspace guarantees the generator matches the scaffolding.

fn main() {
    uniffi::uniffi_bindgen_main()
}
