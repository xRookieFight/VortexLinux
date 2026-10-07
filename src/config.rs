use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunnerKind {
    Wine,
    Proton,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub runner: RunnerKind,
    /// Wine binary, either a name looked up in PATH or a full path.
    pub wine: PathBuf,
    /// Proton install dir, the one that contains the `proton` script.
    pub proton: Option<PathBuf>,
    pub check_updates_on_launch: bool,
    /// Extra environment for the game, e.g. `DXVK_HUD` or `WINEDEBUG`.
    pub env: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            runner: RunnerKind::Wine,
            wine: PathBuf::from("wine"),
            proton: None,
            check_updates_on_launch: false,
            env: BTreeMap::new(),
        }
    }
}

pub const TEMPLATE: &str = r#"# VortexLinux settings. Every key is optional.

# "wine" uses the wine binary below, "proton" needs `proton` set.
runner = "wine"
wine = "wine"
# proton = "/home/you/.steam/steam/steamapps/common/Proton - Experimental"

# Vortex updates itself, turn this on if that ever breaks under Wine.
check_updates_on_launch = false

[env]
# WINEDEBUG = "-all"
"#;

impl Config {
    /// Reads the config, falling back to defaults when the file doesn't exist yet.
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => {
                Self::parse(&text).with_context(|| format!("bad config in {}", path.display()))
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("can't read {}", path.display())),
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        Ok(toml::from_str(text)?)
    }

    /// Drops the commented template in place, but never touches an existing file.
    pub fn write_template(path: &Path) -> Result<bool> {
        if path.exists() {
            return Ok(false);
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, TEMPLATE)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_parses_to_defaults() {
        assert_eq!(Config::parse(TEMPLATE).unwrap(), Config::default());
    }

    #[test]
    fn proton_config() {
        let cfg = Config::parse(
            r#"
            runner = "proton"
            proton = "/opt/proton"
            [env]
            DXVK_HUD = "fps"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.runner, RunnerKind::Proton);
        assert_eq!(cfg.proton.as_deref(), Some(Path::new("/opt/proton")));
        assert_eq!(cfg.env["DXVK_HUD"], "fps");
    }

    #[test]
    fn typos_are_errors() {
        assert!(Config::parse("runer = \"wine\"").is_err());
        assert!(Config::parse("runner = \"dosbox\"").is_err());
    }

    #[test]
    fn missing_file_means_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load(&dir.path().join("nope.toml")).unwrap();
        assert_eq!(cfg, Config::default());
    }
}
