use std::process::Command;

use crate::app::App;
use crate::config::Config;
use crate::paths::{self, Paths};
use crate::runner::{self, Runner};
use crate::updater;

/// Prints one line per check and returns whether everything needed to play is fine.
pub fn run(paths: &Paths) -> bool {
    let mut ok = true;
    let mut report = |good: bool, what: &str, detail: String| {
        ok &= good;
        println!("[{}] {what}: {detail}", if good { " ok " } else { "FAIL" });
    };

    let cfg = match Config::load(&paths.config_file()) {
        Ok(cfg) => {
            report(true, "config", paths.config_file().display().to_string());
            cfg
        }
        Err(e) => {
            report(false, "config", format!("{e:#}"));
            return false;
        }
    };

    let runner = Runner::resolve(&cfg);
    match &runner {
        Ok(r) => report(true, "runner", r.describe()),
        Err(e) => report(false, "runner", format!("{e:#}")),
    }

    if let Ok(Runner::Wine { wine }) = &runner {
        if let Some(dir) = runner::missing_wine_data(wine) {
            report(
                false,
                "wine data",
                format!(
                    "{} is missing, prefix setup will hang. On Fedora: sudo dnf install wine-common",
                    dir.display()
                ),
            );
        }
        match Command::new(wine).arg("--version").output() {
            Ok(out) => report(
                out.status.success(),
                "wine version",
                String::from_utf8_lossy(&out.stdout).trim().to_owned(),
            ),
            Err(e) => report(false, "wine version", e.to_string()),
        }
    }

    // Vortex can fall back to GL, so missing Vulkan is a warning more than a failure,
    // but it's by far the most common reason for a black window
    match Command::new("vulkaninfo").arg("--summary").output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let gpus: Vec<&str> = text
                .lines()
                .filter_map(|l| l.trim().strip_prefix("deviceName"))
                .map(|l| l.trim_start_matches([' ', '=']).trim())
                .collect();
            report(true, "vulkan", gpus.join(", "));
        }
        Ok(_) => report(
            false,
            "vulkan",
            "vulkaninfo failed, check your GPU drivers".into(),
        ),
        Err(_) => println!("[ ?? ] vulkan: vulkaninfo isn't installed, can't check"),
    }

    for app in App::ALL {
        let exe = paths.app_exe(app);
        let version = updater::installed_version(paths, app);
        match (exe.is_file(), app) {
            (true, _) => report(
                true,
                app.name(),
                format!(
                    "{} at {}",
                    version.as_deref().unwrap_or("unknown version"),
                    exe.display()
                ),
            ),
            (false, App::Client) => report(
                false,
                app.name(),
                "not installed, run `vortexlinux update`".into(),
            ),
            // Studio is optional, missing it isn't a problem
            (false, App::Studio) => println!(
                "[ -- ] {}: not installed, `vortexlinux install --studio` adds it",
                app.name()
            ),
        }
    }

    let prefix = paths.wine_prefix();
    if prefix.join(runner::PREFIX_READY).exists() {
        report(true, "wine prefix", prefix.display().to_string());
    } else {
        println!("[ -- ] wine prefix: not created yet, first `run` will do it");
    }

    match paths::find_in_path("vortexlinux") {
        Some(p) => println!("[ ok ] on PATH: {}", p.display()),
        None => println!(
            "[ -- ] on PATH: no, add {} to PATH for the CLI",
            paths.bin_dir.display()
        ),
    }

    ok
}
