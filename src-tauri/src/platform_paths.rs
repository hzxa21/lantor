use std::{env, path::PathBuf};

const APP_DIR_NAME: &str = "Lantor";

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
}

fn legacy_app_data_dir() -> Option<PathBuf> {
    home_dir().map(|home| {
        home.join("Library")
            .join("Application Support")
            .join(APP_DIR_NAME)
    })
}

pub(crate) fn expand_home_path(value: &str) -> String {
    let value = value.trim();
    if value == "~" {
        return env::var("HOME").unwrap_or_else(|_| value.to_owned());
    }
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    value.to_owned()
}

#[cfg(target_os = "macos")]
fn default_app_data_dir() -> PathBuf {
    legacy_app_data_dir().unwrap_or_else(|| {
        PathBuf::from("~")
            .join("Library")
            .join("Application Support")
            .join(APP_DIR_NAME)
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn default_app_data_dir() -> PathBuf {
    env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME.to_ascii_lowercase())
}

#[cfg(windows)]
fn default_app_data_dir() -> PathBuf {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join("AppData").join("Roaming")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
}

#[cfg(not(any(unix, windows)))]
fn default_app_data_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
}

fn compatible_default_path(xdg_path: PathBuf, legacy_path: Option<PathBuf>) -> PathBuf {
    let Some(legacy_path) = legacy_path else {
        return xdg_path;
    };
    if legacy_path.exists() && !xdg_path.exists() {
        return legacy_path;
    }
    xdg_path
}

#[cfg(all(unix, not(target_os = "macos")))]
fn default_database_path() -> PathBuf {
    compatible_default_path(
        default_app_data_dir().join("lantor.sqlite"),
        legacy_app_data_dir().map(|dir| dir.join("lantor.sqlite")),
    )
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn default_database_path() -> PathBuf {
    default_app_data_dir().join("lantor.sqlite")
}

pub(crate) fn default_database_url() -> String {
    format!("sqlite://{}", default_database_path().to_string_lossy())
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn default_attachment_dir() -> PathBuf {
    compatible_default_path(
        default_app_data_dir().join("attachments"),
        legacy_app_data_dir().map(|dir| dir.join("attachments")),
    )
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub(crate) fn default_attachment_dir() -> PathBuf {
    default_app_data_dir().join("attachments")
}

fn executable_exists(path: &str) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub(crate) fn script_shell() -> (PathBuf, Vec<&'static str>) {
    (PathBuf::from("/bin/zsh"), vec!["-lc"])
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn script_shell() -> (PathBuf, Vec<&'static str>) {
    if let Some(shell) = env::var_os("SHELL").and_then(|value| {
        let path = PathBuf::from(value);
        let name = path.file_name()?.to_str()?;
        matches!(name, "bash" | "zsh").then_some(path)
    }) {
        if executable_exists(&shell.to_string_lossy()) {
            return (shell, vec!["-lc"]);
        }
    }

    if executable_exists("/bin/bash") {
        return (PathBuf::from("/bin/bash"), vec!["-lc"]);
    }

    (PathBuf::from("/bin/sh"), vec!["-c"])
}

#[cfg(windows)]
pub(crate) fn script_shell() -> (PathBuf, Vec<&'static str>) {
    (PathBuf::from("cmd"), vec!["/C"])
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn script_shell() -> (PathBuf, Vec<&'static str>) {
    (PathBuf::from("sh"), vec!["-c"])
}

#[cfg(test)]
mod tests {
    use super::{compatible_default_path, expand_home_path};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        std::env::temp_dir().join(format!("lantor-platform-paths-{nanos}-{name}"))
    }

    #[test]
    fn expands_home_prefixes() {
        let home = std::env::var("HOME").expect("HOME should be set for tests");
        assert_eq!(expand_home_path("~"), home);
        assert!(expand_home_path("~/example").ends_with("example"));
    }

    #[test]
    fn compatible_default_path_uses_legacy_only_when_new_path_is_missing() {
        let root = temp_path("compat");
        let xdg_path = root.join("xdg").join("lantor.sqlite");
        let legacy_path = root.join("legacy").join("lantor.sqlite");

        fs::create_dir_all(legacy_path.parent().expect("legacy parent")).expect("mkdir legacy");
        fs::write(&legacy_path, "").expect("write legacy");
        assert_eq!(
            compatible_default_path(xdg_path.clone(), Some(legacy_path.clone())),
            legacy_path
        );

        fs::create_dir_all(xdg_path.parent().expect("xdg parent")).expect("mkdir xdg");
        fs::write(&xdg_path, "").expect("write xdg");
        assert_eq!(
            compatible_default_path(xdg_path.clone(), Some(legacy_path)),
            xdg_path
        );

        let _ = fs::remove_dir_all(root);
    }
}
