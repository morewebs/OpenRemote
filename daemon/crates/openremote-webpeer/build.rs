// The wasm build imports one function from the page: the random bytes the
// browser's crypto.getRandomValues supplies. A WebAssembly module may import
// what its instantiator provides, but rust-lld refuses an undefined symbol by
// default, so this build script tells the linker that the import is expected.
//
// The flag is only a wasm linker's: a host build of this crate (the workspace
// tests it on every platform) hands it to cc, which answers "unrecognized
// command-line option" and fails the whole suite. So it is asked for exactly
// where it means something.
fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if target == "wasm32" {
        println!("cargo:rustc-link-arg=--allow-undefined");
    }
}
