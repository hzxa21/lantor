use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command as StdCommand, Stdio},
};

use crate::{
    app::{to_string, CommandResult},
    models::LaunchAgentStatus,
};

const SERVICE_LABEL: &str = "local.lantor.supervisor";

pub(crate) fn spawn_supervisor_process(database_url: &str) {
    let Ok(exe) = env::current_exe() else {
        eprintln!("failed to resolve current executable for Lantor supervisor");
        return;
    };

    if let Err(err) = StdCommand::new(exe)
        .arg("--supervisor")
        .env("LANTOR_DATABASE_URL", database_url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        eprintln!("failed to spawn Lantor supervisor: {err}");
    }
}

pub(crate) fn load_launch_agent_status() -> CommandResult<LaunchAgentStatus> {
    platform_service::load_status()
}

pub(crate) fn install_supervisor_service(database_url: &str) -> CommandResult<LaunchAgentStatus> {
    platform_service::install(database_url)
}

pub(crate) fn uninstall_supervisor_service() -> CommandResult<LaunchAgentStatus> {
    platform_service::uninstall()
}

fn remove_file_if_exists(path: &Path) -> CommandResult<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.to_string()),
    }
    Ok(())
}

#[cfg(target_os = "macos")]
mod platform_service {
    use super::*;

    pub(crate) fn load_status() -> CommandResult<LaunchAgentStatus> {
        let plist_path = launch_agent_plist_path()?;
        let installed = plist_path.exists();
        let loaded = launch_agent_domain()
            .map(|domain| {
                StdCommand::new("launchctl")
                    .arg("print")
                    .arg(launch_agent_service_target(&domain))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map(|status| status.success())
                    .unwrap_or(false)
            })
            .unwrap_or(false);

        Ok(LaunchAgentStatus {
            label: SERVICE_LABEL.to_owned(),
            plist_path: plist_path.to_string_lossy().to_string(),
            installed,
            loaded,
        })
    }

    pub(crate) fn install(database_url: &str) -> CommandResult<LaunchAgentStatus> {
        let plist_path = launch_agent_plist_path()?;
        let exe_path = env::current_exe().map_err(to_string)?;
        let plist = render_launch_agent_plist(&exe_path, database_url);

        if let Some(parent) = plist_path.parent() {
            fs::create_dir_all(parent).map_err(to_string)?;
        }
        fs::write(&plist_path, plist).map_err(to_string)?;

        let domain = launch_agent_domain()?;
        let service = launch_agent_service_target(&domain);
        let _ = StdCommand::new("launchctl")
            .arg("bootout")
            .arg(&service)
            .output();

        run_launchctl(&["bootstrap", &domain, &plist_path.to_string_lossy()])?;
        run_launchctl(&["kickstart", "-k", &service])?;

        load_status()
    }

    pub(crate) fn uninstall() -> CommandResult<LaunchAgentStatus> {
        let domain = launch_agent_domain()?;
        let service = launch_agent_service_target(&domain);
        let _ = StdCommand::new("launchctl")
            .arg("bootout")
            .arg(&service)
            .output();

        remove_file_if_exists(&launch_agent_plist_path()?)?;

        load_status()
    }

    fn launch_agent_plist_path() -> CommandResult<PathBuf> {
        let home = env::var_os("HOME").ok_or_else(|| "HOME is not set".to_owned())?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{SERVICE_LABEL}.plist")))
    }

    fn launch_agent_domain() -> CommandResult<String> {
        let output = StdCommand::new("id")
            .arg("-u")
            .output()
            .map_err(to_string)?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        let uid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if uid.is_empty() {
            return Err("failed to resolve current uid".to_owned());
        }
        Ok(format!("gui/{uid}"))
    }

    fn launch_agent_service_target(domain: &str) -> String {
        format!("{domain}/{SERVICE_LABEL}")
    }

    fn run_launchctl(args: &[&str]) -> CommandResult<()> {
        let output = StdCommand::new("launchctl")
            .args(args)
            .output()
            .map_err(to_string)?;
        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(format!(
            "launchctl {} failed: {}{}",
            args.join(" "),
            stderr.trim(),
            if stdout.trim().is_empty() {
                String::new()
            } else {
                format!(" {}", stdout.trim())
            }
        ))
    }

    fn render_launch_agent_plist(exe_path: &Path, database_url: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>--supervisor</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>LANTOR_DATABASE_URL</key>
    <string>{}</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
            xml_escape(SERVICE_LABEL),
            xml_escape(&exe_path.to_string_lossy()),
            xml_escape(database_url),
            xml_escape("/tmp/lantor-supervisor.out.log"),
            xml_escape("/tmp/lantor-supervisor.err.log"),
        )
    }

    fn xml_escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
}

#[cfg(target_os = "linux")]
mod platform_service {
    use super::*;

    const UNIT_NAME: &str = "local.lantor.supervisor.service";

    pub(crate) fn load_status() -> CommandResult<LaunchAgentStatus> {
        let unit_path = systemd_user_unit_path()?;
        let installed = unit_path.exists();
        let loaded = StdCommand::new("systemctl")
            .args(["--user", "is-active", "--quiet", UNIT_NAME])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);

        Ok(LaunchAgentStatus {
            label: SERVICE_LABEL.to_owned(),
            plist_path: unit_path.to_string_lossy().to_string(),
            installed,
            loaded,
        })
    }

    pub(crate) fn install(database_url: &str) -> CommandResult<LaunchAgentStatus> {
        let unit_path = systemd_user_unit_path()?;
        let exe_path = env::current_exe().map_err(to_string)?;
        let log_dir = systemd_log_dir()?;

        if let Some(parent) = unit_path.parent() {
            fs::create_dir_all(parent).map_err(to_string)?;
        }
        fs::create_dir_all(&log_dir).map_err(to_string)?;
        fs::write(
            &unit_path,
            render_systemd_unit(&exe_path, database_url, &log_dir),
        )
        .map_err(to_string)?;

        run_systemctl(&["--user", "daemon-reload"])?;
        run_systemctl(&["--user", "enable", "--now", UNIT_NAME])?;

        load_status()
    }

    pub(crate) fn uninstall() -> CommandResult<LaunchAgentStatus> {
        let _ = StdCommand::new("systemctl")
            .args(["--user", "disable", "--now", UNIT_NAME])
            .output();
        remove_file_if_exists(&systemd_user_unit_path()?)?;
        let _ = StdCommand::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();

        load_status()
    }

    fn systemd_user_unit_path() -> CommandResult<PathBuf> {
        let config_home = env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .ok_or_else(|| "HOME is not set".to_owned())?;
        Ok(config_home.join("systemd").join("user").join(UNIT_NAME))
    }

    fn systemd_log_dir() -> CommandResult<PathBuf> {
        let state_home = env::var_os("XDG_STATE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("state"))
            })
            .ok_or_else(|| "HOME is not set".to_owned())?;
        Ok(state_home.join("lantor"))
    }

    fn run_systemctl(args: &[&str]) -> CommandResult<()> {
        let output = StdCommand::new("systemctl")
            .args(args)
            .output()
            .map_err(to_string)?;
        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(format!(
            "systemctl {} failed: {}{}",
            args.join(" "),
            stderr.trim(),
            if stdout.trim().is_empty() {
                String::new()
            } else {
                format!(" {}", stdout.trim())
            }
        ))
    }

    fn render_systemd_unit(exe_path: &Path, database_url: &str, log_dir: &Path) -> String {
        format!(
            r#"[Unit]
Description=Lantor supervisor
After=default.target

[Service]
Type=simple
ExecStart={} --supervisor
Environment={}
Restart=always
RestartSec=2
StandardOutput=append:{}
StandardError=append:{}

[Install]
WantedBy=default.target
"#,
            systemd_quote(&exe_path.to_string_lossy()),
            systemd_quote(&format!("LANTOR_DATABASE_URL={database_url}")),
            log_dir.join("supervisor.out.log").to_string_lossy(),
            log_dir.join("supervisor.err.log").to_string_lossy(),
        )
    }

    fn systemd_quote(value: &str) -> String {
        let escaped = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('`', "\\`");
        format!("\"{escaped}\"")
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform_service {
    use super::*;

    pub(crate) fn load_status() -> CommandResult<LaunchAgentStatus> {
        Ok(LaunchAgentStatus {
            label: SERVICE_LABEL.to_owned(),
            plist_path: String::new(),
            installed: false,
            loaded: false,
        })
    }

    pub(crate) fn install(_database_url: &str) -> CommandResult<LaunchAgentStatus> {
        Err("supervisor service install is only supported on macOS and Linux".to_owned())
    }

    pub(crate) fn uninstall() -> CommandResult<LaunchAgentStatus> {
        load_status()
    }
}
