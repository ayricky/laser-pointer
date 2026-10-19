fn main() {
    // Common Controls v6 gives the flyout's checkboxes and sliders the current Windows look.
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest::embed_manifest(embed_manifest::new_manifest("LaserPointer"))
            .expect("unable to embed manifest");
        winresource::WindowsResource::new()
            .set_icon_with_id("assets/brand/app.ico", "1")
            .set("ProductName", "Laser Pointer")
            .set("FileDescription", "Laser Pointer")
            .set("CompanyName", "Ricardo Mariano")
            .set("LegalCopyright", "© 2026 Ricardo Mariano. MIT licence.")
            .set("OriginalFilename", "laser-pointer.exe")
            .set("FileVersion", env!("CARGO_PKG_VERSION"))
            .set("ProductVersion", env!("CARGO_PKG_VERSION"))
            .compile()
            .expect("unable to embed icon and version resources");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/brand/app.ico");
}
