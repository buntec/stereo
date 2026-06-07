fn main() {
    // Ensure the frontend output directory exists so rust-embed compiles even
    // when the frontend hasn't been built yet.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let statics = std::path::Path::new(&manifest_dir).join("dist");
    if !statics.exists() {
        std::fs::create_dir_all(&statics).unwrap();
    }
}
