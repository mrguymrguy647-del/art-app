//! Build script: Cargo runs this before compiling the app.
//!
//! On Windows it puts the icon and version details into ArtApp.exe, so File
//! Explorer, the taskbar and desktop shortcuts show them.

fn main() {
    // Only run again when the icon or the version number changes.
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=../../Cargo.toml");

    // Only Windows programs carry an icon like this.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/icon.ico");
    resource.set("ProductName", "Art App");
    resource.set("FileDescription", "Art App");
    if let Err(error) = resource.compile() {
        // A missing icon is not worth failing the whole build over.
        println!("cargo:warning=Could not add the icon to ArtApp.exe: {error}");
    }
}
