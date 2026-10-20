fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        // Common Controls v6 gives the flyout's dialogs the current Windows look.
        embed_manifest::embed_manifest(embed_manifest::new_manifest("LaserPointer"))
            .expect("unable to embed manifest");
        // The icon Explorer, the taskbar and shortcuts show, and the details on the file's
        // Properties sheet. The version comes from Cargo.toml.
        let mut resources = winresource::WindowsResource::new();
        resources
            .set_icon("assets/brand/app.ico")
            .set("FileDescription", "Laser Pointer")
            .set("ProductName", "Laser Pointer")
            .set("LegalCopyright", "\u{a9} 2026 Ricardo Mariano");
        resources
            .compile()
            .expect("unable to compile the Windows resources");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/brand/app.ico");
}
