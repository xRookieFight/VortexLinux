use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::app::App;
use crate::config::{Config, RunnerKind};
use crate::paths::{self, Paths};

// winemenubuilder would sprinkle wine-extension-*.desktop files over the
// user's menu and steal file associations, none of which Vortex needs.
// d3d12 goes too, see the WGPU_BACKEND note in `command` for why.
const DLL_OVERRIDES: &str = "winemenubuilder.exe=d;d3d12,d3d12core=";
pub const PREFIX_READY: &str = ".vortexlinux-ready";
// a healthy wineboot takes well under a minute, this only catches real hangs
const WINEBOOT_TIMEOUT: Duration = Duration::from_secs(300);

/// The thing that actually turns a Vortex exe into a Linux process.
#[derive(Debug, Clone, PartialEq)]
pub enum Runner {
    Wine { wine: PathBuf },
    Proton { dir: PathBuf, umu: Option<PathBuf> },
}

impl Runner {
    /// Picks the runner from config and makes sure its binaries are really there.
    pub fn resolve(cfg: &Config) -> Result<Self> {
        match cfg.runner {
            RunnerKind::Wine => {
                let wine = resolve_program(&cfg.wine).with_context(|| {
                    format!(
                        "wine not found ({}), install it or set `wine` in the config. `vortexlinux doctor` has details",
                        cfg.wine.display()
                    )
                })?;
                Ok(Self::Wine { wine })
            }
            RunnerKind::Proton => {
                let dir = cfg
                    .proton
                    .clone()
                    .context("runner is \"proton\" but `proton` isn't set in the config")?;
                if !paths::is_executable(&dir.join("proton")) {
                    bail!(
                        "{} doesn't look like a Proton dir, there's no `proton` script in it",
                        dir.display()
                    );
                }
                Ok(Self::Proton {
                    dir,
                    umu: paths::find_in_path("umu-run"),
                })
            }
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::Wine { wine } => format!("wine ({})", wine.display()),
            Self::Proton { dir, umu: Some(_) } => format!("proton via umu-run ({})", dir.display()),
            Self::Proton { dir, umu: None } => format!("proton ({})", dir.display()),
        }
    }

    /// One time prefix setup. Proton builds its own prefix on first launch, so
    /// only plain Wine needs a nudge here.
    pub fn prepare(&self, paths: &Paths, cfg: &Config) -> Result<()> {
        match self {
            Self::Wine { wine } => {
                let prefix = paths.wine_prefix();
                // system.reg shows up early in wineboot, so it can't tell a half
                // built prefix from a finished one. Our own marker can.
                let marker = prefix.join(PREFIX_READY);
                if marker.exists() {
                    return Ok(());
                }
                if let Some(dir) = missing_wine_data(wine) {
                    bail!(
                        "your Wine install is missing {}, prefix setup would hang forever. \
                         On Fedora run `sudo dnf install wine-common`",
                        dir.display()
                    );
                }
                fs::create_dir_all(&prefix)?;
                eprintln!("setting up the Wine prefix, only happens once...");
                let mut child = Command::new(wine)
                    .args(["wineboot", "-u"])
                    .env("WINEPREFIX", &prefix)
                    // Vortex is native Rust, no point in the Mono and Gecko install prompts
                    .env(
                        "WINEDLLOVERRIDES",
                        format!("mscoree,mshtml=;{DLL_OVERRIDES}"),
                    )
                    .envs(&cfg.env)
                    .spawn()
                    .context("failed to start wineboot")?;

                let deadline = Instant::now() + WINEBOOT_TIMEOUT;
                let status = loop {
                    if let Some(status) = child.try_wait()? {
                        break status;
                    }
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        kill_wineserver(wine, &prefix);
                        bail!(
                            "Wine prefix setup didn't finish in {} minutes, gave up. \
                             `vortexlinux doctor` may tell you why",
                            WINEBOOT_TIMEOUT.as_secs() / 60
                        );
                    }
                    thread::sleep(Duration::from_millis(250));
                };
                if !status.success() {
                    bail!(
                        "wineboot failed ({status}), prefix is at {}",
                        prefix.display()
                    );
                }
                fs::write(&marker, "")?;
                Ok(())
            }
            Self::Proton { .. } => {
                fs::create_dir_all(paths.compat_data())?;
                Ok(())
            }
        }
    }

    /// Builds the launch command. `url` should already have gone through [`App::validate_url`].
    pub fn command(&self, paths: &Paths, cfg: &Config, app: App, url: Option<&str>) -> Command {
        let exe = paths.app_exe(app);
        let mut cmd = match self {
            Self::Wine { wine } => {
                let mut c = Command::new(wine);
                c.arg(&exe).env("WINEPREFIX", paths.wine_prefix());
                c
            }
            Self::Proton {
                dir,
                umu: Some(umu),
            } => {
                let mut c = Command::new(umu);
                c.arg(&exe)
                    .env("PROTONPATH", dir)
                    .env("WINEPREFIX", paths.compat_data().join("pfx"))
                    .env("GAMEID", "umu-default");
                c
            }
            Self::Proton { dir, umu: None } => {
                let mut c = Command::new(dir.join("proton"));
                c.arg("run")
                    .arg(&exe)
                    .env("STEAM_COMPAT_DATA_PATH", paths.compat_data())
                    .env(
                        "STEAM_COMPAT_CLIENT_INSTALL_PATH",
                        steam_root().unwrap_or_else(|| paths.data_dir.clone()),
                    );
                c
            }
        };

        if let Some(url) = url {
            cmd.arg(url);
        }
        let defaults = [
            ("WINEDEBUG", "-all"),
            ("WINEDLLOVERRIDES", DLL_OVERRIDES),
            // wgpu prefers DX12, which on Wine means vkd3d reporting a fake
            // "HD Graphics 4000" and Vortex panicking on its first texture.
            // Native Vulkan through winevulkan just works. The client honors
            // this var, Studio ignores it, hence d3d12 being switched off above.
            ("WGPU_BACKEND", "vulkan"),
        ];
        for (key, value) in defaults {
            if env::var_os(key).is_none() && !cfg.env.contains_key(key) {
                cmd.env(key, value);
            }
        }
        cmd.envs(&cfg.env).current_dir(paths.app_dir(app));
        cmd
    }
}

/// Finds Wine's data dir from the binary and checks the `.winmd` files that
/// `wine.inf` copies are actually there. Fedora ships them in `wine-common`, and
/// without them setupapi retries the copy forever while the "updating Wine
/// configuration" dialog spins. Returns the missing dir.
pub fn missing_wine_data(wine: &Path) -> Option<PathBuf> {
    let real = fs::canonicalize(wine).ok()?;
    let data = real.parent()?.parent()?.join("share/wine");
    let inf = fs::read_to_string(data.join("wine.inf")).ok()?;
    let winmd = data.join("winmd");
    (inf.contains("winmd") && !winmd.is_dir()).then_some(winmd)
}

fn kill_wineserver(wine: &Path, prefix: &Path) {
    let sibling = wine.with_file_name("wineserver");
    let server = if paths::is_executable(&sibling) {
        sibling
    } else {
        PathBuf::from("wineserver")
    };
    let _ = Command::new(server)
        .arg("-k")
        .env("WINEPREFIX", prefix)
        .status();
}

fn resolve_program(program: &Path) -> Option<PathBuf> {
    if program.components().count() == 1 {
        program.to_str().and_then(paths::find_in_path)
    } else {
        paths::is_executable(program).then(|| program.to_path_buf())
    }
}

// proton refuses to start without STEAM_COMPAT_CLIENT_INSTALL_PATH, but any dir
// works when Steam isn't installed at all
fn steam_root() -> Option<PathBuf> {
    let home = PathBuf::from(env::var_os("HOME")?);
    [".steam/steam", ".local/share/Steam"]
        .iter()
        .map(|p| home.join(p))
        .find(|p| p.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn paths() -> Paths {
        Paths::new(
            Path::new("/h"),
            PathBuf::from("/h/.config"),
            PathBuf::from("/h/.local/share"),
        )
    }

    fn env_of<'a>(cmd: &'a Command, key: &str) -> Option<&'a OsStr> {
        cmd.get_envs()
            .find(|(k, _)| *k == OsStr::new(key))
            .and_then(|(_, v)| v)
    }

    fn args_of(cmd: &Command) -> Vec<&OsStr> {
        cmd.get_args().collect()
    }

    #[test]
    fn wine_command() {
        let p = paths();
        let mut cfg = Config::default();
        cfg.env.insert("DXVK_HUD".into(), "fps".into());
        let r = Runner::Wine {
            wine: "/usr/bin/wine".into(),
        };
        let cmd = r.command(&p, &cfg, App::Client, Some("vortex://event/123"));

        assert_eq!(cmd.get_program(), "/usr/bin/wine");
        assert_eq!(
            args_of(&cmd),
            [
                p.app_exe(App::Client).as_os_str(),
                OsStr::new("vortex://event/123")
            ]
        );
        assert_eq!(
            env_of(&cmd, "WINEPREFIX"),
            Some(p.wine_prefix().as_os_str())
        );
        assert_eq!(env_of(&cmd, "DXVK_HUD"), Some(OsStr::new("fps")));
        assert_eq!(
            cmd.get_current_dir(),
            Some(p.app_dir(App::Client).as_path())
        );
    }

    #[test]
    fn defaults_to_vulkan_without_d3d12_or_menubuilder() {
        let cmd = Runner::Wine {
            wine: "wine".into(),
        }
        .command(&paths(), &Config::default(), App::Client, None);
        assert_eq!(env_of(&cmd, "WGPU_BACKEND"), Some(OsStr::new("vulkan")));
        assert_eq!(
            env_of(&cmd, "WINEDLLOVERRIDES"),
            Some(OsStr::new("winemenubuilder.exe=d;d3d12,d3d12core="))
        );
    }

    #[test]
    fn studio_runs_its_own_exe() {
        let p = paths();
        let r = Runner::Wine {
            wine: "wine".into(),
        };
        let cmd = r.command(
            &p,
            &Config::default(),
            App::Studio,
            Some("vortex-studio://x"),
        );
        assert_eq!(
            args_of(&cmd),
            [
                p.app_exe(App::Studio).as_os_str(),
                OsStr::new("vortex-studio://x")
            ]
        );
        assert_eq!(
            cmd.get_current_dir(),
            Some(p.app_dir(App::Studio).as_path())
        );
        // one prefix for both, so Studio sees the same AppData as the client
        assert_eq!(
            env_of(&cmd, "WINEPREFIX"),
            Some(p.wine_prefix().as_os_str())
        );
    }

    #[test]
    fn user_winedebug_wins() {
        let mut cfg = Config::default();
        cfg.env.insert("WINEDEBUG".into(), "+loaddll".into());
        let r = Runner::Wine {
            wine: "wine".into(),
        };
        let cmd = r.command(&paths(), &cfg, App::Client, None);
        assert_eq!(env_of(&cmd, "WINEDEBUG"), Some(OsStr::new("+loaddll")));
    }

    #[test]
    fn proton_with_umu() {
        let p = paths();
        let r = Runner::Proton {
            dir: "/opt/GE".into(),
            umu: Some("/usr/bin/umu-run".into()),
        };
        let cmd = r.command(&p, &Config::default(), App::Client, None);

        assert_eq!(cmd.get_program(), "/usr/bin/umu-run");
        assert_eq!(args_of(&cmd), [p.app_exe(App::Client).as_os_str()]);
        assert_eq!(env_of(&cmd, "PROTONPATH"), Some(OsStr::new("/opt/GE")));
        assert_eq!(
            env_of(&cmd, "WINEPREFIX"),
            Some(p.compat_data().join("pfx").as_os_str())
        );
    }

    #[test]
    fn proton_without_umu() {
        let p = paths();
        let r = Runner::Proton {
            dir: "/opt/GE".into(),
            umu: None,
        };
        let cmd = r.command(&p, &Config::default(), App::Client, None);

        assert_eq!(cmd.get_program(), "/opt/GE/proton");
        assert_eq!(
            args_of(&cmd),
            [OsStr::new("run"), p.app_exe(App::Client).as_os_str()]
        );
        assert_eq!(
            env_of(&cmd, "STEAM_COMPAT_DATA_PATH"),
            Some(p.compat_data().as_os_str())
        );
        assert!(env_of(&cmd, "STEAM_COMPAT_CLIENT_INSTALL_PATH").is_some());
    }

    #[test]
    fn spots_missing_winmd() {
        let root = tempfile::tempdir().unwrap();
        let wine = root.path().join("bin/wine");
        let data = root.path().join("share/wine");
        fs::create_dir_all(wine.parent().unwrap()).unwrap();
        fs::create_dir_all(&data).unwrap();
        fs::write(&wine, "").unwrap();
        fs::write(
            data.join("wine.inf"),
            "[WinmdFiles]\nwindows.foundation.winmd\n",
        )
        .unwrap();

        assert_eq!(
            missing_wine_data(&wine),
            Some(fs::canonicalize(&data).unwrap().join("winmd"))
        );
        fs::create_dir(data.join("winmd")).unwrap();
        assert_eq!(missing_wine_data(&wine), None);
    }

    #[test]
    fn proton_needs_a_dir() {
        let cfg = Config {
            runner: RunnerKind::Proton,
            ..Config::default()
        };
        assert!(Runner::resolve(&cfg).is_err());

        let dir = tempfile::tempdir().unwrap();
        let cfg = Config {
            runner: RunnerKind::Proton,
            proton: Some(dir.path().into()),
            ..Config::default()
        };
        assert!(Runner::resolve(&cfg).is_err());
    }
}
