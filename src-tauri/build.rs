//! Release builds ask for administrator rights (SMART and, later, the driver need them).
//! Debug builds keep Tauri's default manifest so `pnpm tauri dev` runs from a normal
//! terminal; the app then degrades to its unelevated view.

fn main() {
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    let mut attrs = tauri_build::Attributes::new();
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        attrs = attrs.windows_attributes(
            tauri_build::WindowsAttributes::new()
                .app_manifest(include_str!("windows-app-manifest.xml")),
        );
    }
    if let Err(e) = tauri_build::try_build(attrs) {
        panic!("{e:#}");
    }
}
