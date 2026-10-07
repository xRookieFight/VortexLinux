mod app;
mod config;
mod desktop;
mod doctor;
mod paths;
mod runner;
mod updater;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use app::App;
use config::Config;
use paths::Paths;
use runner::Runner;
use updater::Outcome;

/// Run the Windows Vortex client and Studio on Linux through Wine or Proton.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Install the launcher, download Vortex and add it to the app menu.
    Install {
        /// Also install Vortex Studio.
        #[arg(long)]
        studio: bool,
    },
    /// Start Vortex, optionally opening a vortex:// link.
    Run {
        /// Start Vortex Studio instead, links are then vortex-studio://
        #[arg(long)]
        studio: bool,
        url: Option<String>,
    },
    /// Download the latest version if ours is out of date.
    Update {
        /// Update Vortex Studio instead of the client.
        #[arg(long)]
        studio: bool,
        /// Redownload even if the version matches.
        #[arg(long)]
        force: bool,
    },
    /// Remove the menu entries and launcher.
    Uninstall {
        /// Only remove Vortex Studio and keep the client.
        #[arg(long)]
        studio: bool,
        /// Also delete downloaded files. With --studio only Studio's, otherwise
        /// the client, Studio, the Wine prefix and the config.
        #[arg(long)]
        purge: bool,
    },
    /// Check that everything needed to play is in place.
    Doctor,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = Paths::from_env().and_then(|paths| match cli.command {
        Cmd::Install { studio } => install(&paths, studio),
        Cmd::Run { studio, url } => run(&paths, App::from_flag(studio), url.as_deref()),
        Cmd::Update { studio, force } => update(&paths, App::from_flag(studio), force),
        Cmd::Uninstall {
            studio: true,
            purge,
        } => uninstall_studio(&paths, purge),
        Cmd::Uninstall {
            studio: false,
            purge,
        } => uninstall(&paths, purge),
        Cmd::Doctor => {
            if doctor::run(&paths) {
                Ok(())
            } else {
                bail!("some checks failed")
            }
        }
    });

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn install(paths: &Paths, studio: bool) -> Result<()> {
    let target = paths.installed_bin();
    let current = std::env::current_exe().context("can't find our own binary")?;
    if fs::canonicalize(&current).ok() != fs::canonicalize(&target).ok() {
        fs::create_dir_all(&paths.bin_dir)?;
        let part = target.with_extension("part");
        fs::copy(&current, &part).with_context(|| format!("can't copy to {}", part.display()))?;
        fs::set_permissions(&part, fs::Permissions::from_mode(0o755))?;
        fs::rename(&part, &target)?;
        println!("installed launcher to {}", target.display());
    }

    if Config::write_template(&paths.config_file())? {
        println!("wrote default config to {}", paths.config_file().display());
    }

    // rerunning plain `install` keeps an existing Studio and refreshes its entry too
    let studio = studio || paths.app_exe(App::Studio).is_file();
    for app in App::ALL {
        if app == App::Studio && !studio {
            continue;
        }
        update(paths, app, false)?;
        desktop::install(paths, app, &target)?;
        println!(
            "added {} to the app menu, {}:// links now open here",
            app.name(),
            app.scheme()
        );
    }

    if paths::find_in_path("vortexlinux").is_none() {
        println!(
            "note: {} isn't on PATH, the menu entry works anyway",
            paths.bin_dir.display()
        );
    }
    Ok(())
}

fn run(paths: &Paths, app: App, url: Option<&str>) -> Result<()> {
    if let Some(url) = url {
        app.validate_url(url)?;
    }
    let cfg = Config::load(&paths.config_file())?;
    let runner = Runner::resolve(&cfg)?;

    if !paths.app_exe(app).is_file() {
        update(paths, app, false)?;
    } else if cfg.check_updates_on_launch {
        // being offline shouldn't stop you from playing what's already installed
        if let Err(e) = update(paths, app, false) {
            eprintln!("warning: update check failed, launching anyway: {e:#}");
        }
    }

    runner.prepare(paths, &cfg)?;
    let err = runner.command(paths, &cfg, app, url).exec();
    Err(err).with_context(|| format!("failed to start {}", app.name()))
}

fn update(paths: &Paths, app: App, force: bool) -> Result<()> {
    let name = app.name();
    match updater::update(paths, app, force)? {
        Outcome::UpToDate(v) => println!("{name} {v} is up to date"),
        Outcome::Updated {
            from: Some(old),
            to,
        } => println!("updated {name} {old} -> {to}"),
        Outcome::Updated { from: None, to } => println!("installed {name} {to}"),
    }
    Ok(())
}

fn uninstall_studio(paths: &Paths, purge: bool) -> Result<()> {
    desktop::uninstall(paths, App::Studio)?;
    println!("removed Vortex Studio from the app menu");
    if purge {
        remove_if_exists(&paths.app_dir(App::Studio), true)?;
        println!("deleted the Studio files");
    }
    Ok(())
}

fn uninstall(paths: &Paths, purge: bool) -> Result<()> {
    for app in App::ALL {
        desktop::uninstall(paths, app)?;
    }
    remove_if_exists(&paths.installed_bin(), false)?;
    println!("removed the menu entry and launcher");

    if purge {
        remove_if_exists(&paths.data_dir, true)?;
        remove_if_exists(&paths.config_dir, true)?;
        println!("deleted the client, Studio, prefix and config");
    } else {
        println!(
            "downloads and prefix kept in {}, use --purge to delete them",
            paths.data_dir.display()
        );
    }
    Ok(())
}

fn remove_if_exists(path: &std::path::Path, dir: bool) -> Result<()> {
    let res = if dir {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    match res {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("can't remove {}", path.display()))
        }
        _ => Ok(()),
    }
}
