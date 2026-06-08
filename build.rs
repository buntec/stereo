fn main() {
    // Re-run when git state changes
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/");

    let git_version = std::process::Command::new("git")
        .args(["describe", "--tags", "--dirty", "--always"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=GIT_VERSION={git_version}");

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
