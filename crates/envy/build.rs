fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/envy.ico");
    println!("cargo:rerun-if-changed=../../assets/icon/envy.rc");
    println!("cargo:rerun-if-changed=../../assets/icon/envy-icon-32.png");
    println!("cargo:rerun-if-changed=../../assets/icon/envy-icon-64.png");
    #[cfg(windows)]
    embed_icon();
}

/// Write an rc script with an absolute icon path so `rc.exe` and `windres`
/// both find `assets/icon/envy.ico` no matter which directory they start in.
#[cfg(windows)]
fn embed_icon() {
    let manifest =
        std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let ico = manifest.join("../../assets/icon/envy.ico");
    let mut ico = ico.canonicalize().unwrap_or(ico).display().to_string();
    if let Some(rest) = ico.strip_prefix(r"\\?\") {
        ico = rest.to_string();
    }
    let ico = ico.replace('\\', "/");
    let rc_path =
        std::path::PathBuf::from(std::env::var("OUT_DIR").expect("out dir")).join("envy.rc");
    std::fs::write(&rc_path, format!("1 ICON \"{ico}\"\n")).expect("write icon script");
    embed_resource::compile(&rc_path, embed_resource::NONE);
}
