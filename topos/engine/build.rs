//! Link OpenSSL for the embedded `lbug` (LadybugDB) C++ client on platforms
//! that don't pull it in transitively.

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    // The embedded `lbug` C++ client links OpenSSL on Unix targets. Windows
    // uses a different crypto backend, so linking `ssl`/`crypto` there fails.
    if target_os == "macos" || target_os == "linux" {
        if let Ok(dir) = std::env::var("OPENSSL_DIR") {
            let lib = format!("{dir}/lib");
            println!("cargo:rustc-link-search=native={lib}");
        } else if target_os == "macos" {
            if let Some(prefix) = brew_prefix("openssl@3").or_else(|| brew_prefix("openssl")) {
                println!("cargo:rustc-link-search=native={prefix}/lib");
            }
        }
        println!("cargo:rustc-link-lib=ssl");
        println!("cargo:rustc-link-lib=crypto");
    }
    println!("cargo:rerun-if-env-changed=OPENSSL_DIR");
    println!("cargo:rerun-if-changed=build.rs");
}

fn brew_prefix(formula: &str) -> Option<String> {
    let output = std::process::Command::new("brew")
        .args(["--prefix", formula])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let prefix = String::from_utf8(output.stdout).ok()?;
    let prefix = prefix.trim();
    if prefix.is_empty() {
        None
    } else {
        Some(prefix.to_string())
    }
}
