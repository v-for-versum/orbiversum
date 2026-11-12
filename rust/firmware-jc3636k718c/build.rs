use std::process::Command;

fn main() {
    embuild::espidf::sysenv::output();

    // The ESP-IDF app_version is cached by CMake and goes stale; stamp our own.
    let desc = Command::new("git")
        .args(["describe", "--always", "--dirty"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=FW_GIT_DESC={desc}");
    for p in ["../../.git/HEAD", "../../.git/index", "src", "../crates"] {
        println!("cargo:rerun-if-changed={p}");
    }
}
