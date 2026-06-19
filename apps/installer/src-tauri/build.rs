fn main() {
    println!("cargo:rerun-if-changed=../../desktop/src-tauri/icons/icon.ico");
    let windows = tauri_build::WindowsAttributes::new()
        .window_icon_path("../../desktop/src-tauri/icons/icon.ico");
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attributes).expect("failed to build Lan Code installer resources");
}
