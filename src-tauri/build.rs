//! The app runs unelevated (`asInvoker`): the Clokrix sensor service does the reads that need
//! administrator rights (PawnIO, SMART).

fn main() {
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    // Clokrix Pro's checkout (2.5.1): the app drops an address without its scheme (edition.rs,
    // `checkout`), which would leave the build Pro for everyone. Stop the build instead.
    println!("cargo:rerun-if-env-changed=CLOKRIX_LICENCE_URL");
    if let Ok(url) = std::env::var("CLOKRIX_LICENCE_URL") {
        if !url.is_empty() && !(url.starts_with("https://") || url.starts_with("http://")) {
            panic!("CLOKRIX_LICENCE_URL must start with https:// (it is {url:?})");
        }
    }
    let attrs = tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new()
            .app_manifest(include_str!("windows-app-manifest.xml")),
    );
    if let Err(e) = tauri_build::try_build(attrs) {
        panic!("{e:#}");
    }
}
