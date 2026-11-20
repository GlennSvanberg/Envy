//! Register the `envy://` URL scheme.
//!
//! On Windows this writes the current user's `Software\Classes\envy` key so a
//! link opens this executable. On Linux it installs a per-user desktop entry.
//! Failures are reported as text and never include environment values.

pub fn register() -> Result<(), String> {
    #[cfg(windows)]
    {
        register_windows()
    }
    #[cfg(not(windows))]
    {
        register_unix()
    }
}

#[cfg(windows)]
fn register_windows() -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey("Software\\Classes\\envy")
        .map_err(|err| err.to_string())?;
    key.set_value("", &"URL:Envy Protocol")
        .map_err(|err| err.to_string())?;
    key.set_value("URL Protocol", &"")
        .map_err(|err| err.to_string())?;
    let (command, _) = hkcu
        .create_subkey("Software\\Classes\\envy\\shell\\open\\command")
        .map_err(|err| err.to_string())?;
    let value = format!("\"{}\" \"%1\"", exe.display());
    command
        .set_value("", &value)
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(not(windows))]
fn register_unix() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    let apps = std::path::PathBuf::from(&home).join(".local/share/applications");
    std::fs::create_dir_all(&apps).map_err(|err| err.to_string())?;
    let desktop = format!(
        "[Desktop Entry]\nName=Envy\nExec=\"{}\" %u\nType=Application\nTerminal=false\nNoDisplay=true\nMimeType=x-scheme-handler/envy;\n",
        exe.display()
    );
    let desktop_path = apps.join("envy.desktop");
    std::fs::write(&desktop_path, desktop).map_err(|err| err.to_string())?;

    let mime_dir = std::path::PathBuf::from(&home).join(".config");
    let _ = std::fs::create_dir_all(&mime_dir);
    let mime_path = mime_dir.join("mimeapps.list");
    let mut text = std::fs::read_to_string(&mime_path).unwrap_or_default();
    const LINE: &str = "x-scheme-handler/envy=envy.desktop";
    if !text.lines().any(|line| line.trim() == LINE) {
        if !text.contains("[Default Applications]") {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str("[Default Applications]\n");
        }
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(LINE);
        text.push('\n');
        let _ = std::fs::write(&mime_path, text);
    }
    Ok(())
}
