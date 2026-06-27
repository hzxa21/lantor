use std::{env, path::PathBuf};

const APP_DIR_NAME: &str = "Lantor";

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(PathBuf::from)
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
    home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Library")
        .join("Application Support")
        .join(APP_DIR_NAME)
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

pub(crate) fn default_database_url() -> String {
    format!(
        "sqlite://{}",
        default_app_data_dir()
            .join("lantor.sqlite")
            .to_string_lossy()
    )
}

pub(crate) fn default_attachment_dir() -> PathBuf {
    default_app_data_dir().join("attachments")
}

#[cfg(test)]
mod tests {
    use super::expand_home_path;

    #[test]
    fn expands_home_prefixes() {
        let home = std::env::var("HOME").expect("HOME should be set for tests");
        assert_eq!(expand_home_path("~"), home);
        assert!(expand_home_path("~/example").ends_with("example"));
    }
}
