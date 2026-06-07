fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let dist = std::path::Path::new(&manifest_dir).join("dist");

    if !dist.join("index.html").exists() {
        let status = std::process::Command::new("bun")
            .args(["install"])
            .current_dir(&manifest_dir)
            .status()
            .expect("failed to run bun install — is bun installed?");
        assert!(status.success(), "bun install failed");

        let status = std::process::Command::new("bun")
            .args(["run", "build"])
            .current_dir(&manifest_dir)
            .status()
            .expect("failed to run bun run build");
        assert!(status.success(), "bun run build failed");
    }
}
