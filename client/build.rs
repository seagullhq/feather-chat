fn main() {
    println!("cargo:rerun-if-changed=assets/icon.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    // rust-embed bakes icons in release builds: rebuild when one is added.
    println!("cargo:rerun-if-changed=assets/icons");

    embed_resource::compile("assets/icon.rc", embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
