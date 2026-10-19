fn main() {
    // Common Controls v6 gives the flyout's checkboxes and sliders the current Windows look.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest::embed_manifest(embed_manifest::new_manifest("LaserPointer"))
            .expect("unable to embed manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
