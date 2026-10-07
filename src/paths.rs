use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::app::App;

/// Every location VortexLinux reads or writes, resolved once from XDG env vars.
#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub bin_dir: PathBuf,
    pub applications_dir: PathBuf,
    pub icons_dir: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Self> {
        let home = env::var_os("HOME").context("HOME is not set")?;
        let home = PathBuf::from(home);
        if !home.is_absolute() {
            bail!("HOME is not an absolute path");
        }
        Ok(Self::new(
            &home,
            xdg_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
            xdg_dir("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")),
        ))
    }

    pub fn new(home: &Path, config_home: PathBuf, data_home: PathBuf) -> Self {
        Self {
            config_dir: config_home.join("vortexlinux"),
            data_dir: data_home.join("vortexlinux"),
            bin_dir: home.join(".local/bin"),
            applications_dir: data_home.join("applications"),
            icons_dir: data_home.join("icons/hicolor/256x256/apps"),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn app_dir(&self, app: App) -> PathBuf {
        self.data_dir.join(app.dir_name())
    }

    pub fn app_exe(&self, app: App) -> PathBuf {
        self.app_dir(app).join(app.exe_name())
    }

    pub fn version_file(&self, app: App) -> PathBuf {
        self.app_dir(app).join("version")
    }

    pub fn wine_prefix(&self) -> PathBuf {
        self.data_dir.join("prefix")
    }

    pub fn compat_data(&self) -> PathBuf {
        self.data_dir.join("compatdata")
    }

    pub fn installed_bin(&self) -> PathBuf {
        self.bin_dir.join("vortexlinux")
    }

    pub fn desktop_file(&self, app: App) -> PathBuf {
        self.applications_dir.join(app.desktop_id())
    }

    pub fn icon_file(&self, app: App) -> PathBuf {
        self.icons_dir.join(format!("{}.png", app.icon_name()))
    }
}

// the XDG spec says relative values must be ignored, so we do
fn xdg_dir(var: &str) -> Option<PathBuf> {
    env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// Looks a bare program name up in PATH, the way a shell would.
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_follows_xdg_dirs() {
        let p = Paths::new(
            Path::new("/home/u"),
            PathBuf::from("/cfg"),
            PathBuf::from("/data"),
        );
        assert_eq!(p.config_file(), Path::new("/cfg/vortexlinux/config.toml"));
        assert_eq!(
            p.app_exe(App::Client),
            Path::new("/data/vortexlinux/game/Vortex.exe")
        );
        assert_eq!(
            p.app_exe(App::Studio),
            Path::new("/data/vortexlinux/studio/VortexStudio.exe")
        );
        assert_eq!(p.wine_prefix(), Path::new("/data/vortexlinux/prefix"));
        assert_eq!(
            p.installed_bin(),
            Path::new("/home/u/.local/bin/vortexlinux")
        );
        assert_eq!(
            p.desktop_file(App::Client),
            Path::new("/data/applications/vortexlinux.desktop")
        );
        assert_eq!(
            p.icon_file(App::Studio),
            Path::new("/data/icons/hicolor/256x256/apps/vortexlinux-studio.png")
        );
    }
}
