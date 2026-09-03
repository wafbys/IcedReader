fn main() {
    tauri_build::build();

    // 把 build 时的 git 短 hash 固化进二进制（窗口标题展示，便于发布件溯源）。
    // .git 变化时让 cargo 重跑本脚本，保证切 commit / 分支后标题随之更新。
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
    if let Some(hash) = git_short_hash() {
        println!("cargo:rustc-env=ICED_READER_GIT_HASH={hash}");
    }
}

fn git_short_hash() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let hash = String::from_utf8_lossy(&output.stdout);
    let hash = hash.trim();
    if hash.is_empty() {
        None
    } else {
        Some(hash.to_string())
    }
}
