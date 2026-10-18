//! Compiles in data/vectors.bin (ready-made vectors from `superspace precompute`) when it exists.
fn main() {
    let out = std::path::Path::new(&std::env::var_os("OUT_DIR").unwrap()).join("vectors.bin");
    if std::fs::copy("data/vectors.bin", &out).is_err() {
        std::fs::write(&out, b"").unwrap();
    }
}
