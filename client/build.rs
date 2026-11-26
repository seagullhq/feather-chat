fn main() {
    println!("cargo:rerun-if-changed=assets/icon.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    // rust-embed bakes icons in release builds: rebuild when one is added.
    println!("cargo:rerun-if-changed=assets/icons");

    // Swift bridge deps emit their own rpaths, but Cargo does not forward
    // transitive rustc-link-arg to this package's test binaries.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        emit_swift_runtime_rpaths();
    }

    embed_resource::compile("assets/icon.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}

fn emit_swift_runtime_rpaths() {
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::process::Command;

    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");

    let mut roots = BTreeSet::new();
    if let Ok(out) = Command::new("xcode-select").arg("-p").output() {
        if out.status.success() {
            roots.insert(String::from_utf8_lossy(&out.stdout).trim().to_owned());
        }
    }
    if let Ok(out) = Command::new("xcrun").args(["--find", "swiftc"]).output() {
        if out.status.success() {
            let swiftc = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if let Some(usr) = Path::new(&swiftc).parent().and_then(Path::parent) {
                roots.insert(usr.display().to_string());
            }
        }
    }

    let mut dirs = BTreeSet::new();
    for root in roots {
        for rel in [
            "lib/swift/macosx",
            "lib/swift-5.5/macosx",
            "lib/swift-6.2/macosx",
            "Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx",
            "Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx",
            "Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-6.2/macosx",
            "usr/lib/swift/macosx",
            "usr/lib/swift-5.5/macosx",
            "usr/lib/swift-6.2/macosx",
        ] {
            let dir = Path::new(&root).join(rel);
            if dir.is_dir() {
                dirs.insert(dir);
            }
        }
    }

    for dir in dirs {
        println!("cargo:rustc-link-search=native={}", dir.display());
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
    }
}
