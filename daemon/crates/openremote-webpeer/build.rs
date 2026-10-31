// The wasm build imports one function from the page: the random bytes the
// browser's crypto.getRandomValues supplies. A WebAssembly module may import
// what its instantiator provides, but rust-lld refuses an undefined symbol by
// default, so this build script tells the linker that the import is expected.
fn main() {
    println!("cargo:rustc-link-arg=--allow-undefined");
}
