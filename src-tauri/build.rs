//! The app runs unelevated (`asInvoker`): the Vyro sensor service does the reads that need
//! administrator rights (PawnIO, SMART).

fn main() {
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    let attrs = tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new()
            .app_manifest(include_str!("windows-app-manifest.xml")),
    );
    if let Err(e) = tauri_build::try_build(attrs) {
        panic!("{e:#}");
    }
}
