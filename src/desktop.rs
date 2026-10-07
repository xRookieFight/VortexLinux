use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use pelite::pe64::{Pe, PeFile};

use crate::app::App;
use crate::paths::Paths;

/// Writes the app's menu entry and icon, then claims its URL scheme for us.
/// The icon and the xdg calls are best effort, a missing icon shouldn't block install.
pub fn install(paths: &Paths, app: App, bin: &Path) -> Result<()> {
    let desktop_file = paths.desktop_file(app);
    fs::create_dir_all(&paths.applications_dir)?;
    fs::write(&desktop_file, entry(app, bin)?)
        .with_context(|| format!("can't write {}", desktop_file.display()))?;

    if let Err(e) = install_icon(paths, app) {
        eprintln!("warning: couldn't set up the {} icon: {e:#}", app.name());
    }

    let mime = scheme_mime(app);
    run_quietly(Command::new("update-desktop-database").arg(&paths.applications_dir));
    run_quietly(Command::new("xdg-mime").args(["default", app.desktop_id(), &mime]));
    Ok(())
}

pub fn uninstall(paths: &Paths, app: App) -> Result<()> {
    for file in [paths.desktop_file(app), paths.icon_file(app)] {
        match fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(e).with_context(|| format!("can't remove {}", file.display()));
            }
            _ => {}
        }
    }
    // xdg-mime has no "unset", but once the desktop file is gone the
    // stale default is simply ignored
    run_quietly(Command::new("update-desktop-database").arg(&paths.applications_dir));
    Ok(())
}

pub fn entry(app: App, bin: &Path) -> Result<String> {
    let bin = bin.to_str().context("install path isn't valid UTF-8")?;
    // escaping inside Exec= has two layers of rules, easier to just refuse the odd chars
    if bin.contains(['"', '`', '$', '\\', '%', '\n', '\r']) {
        bail!("install path {bin:?} has characters a desktop entry can't express");
    }
    Ok(format!(
        "[Desktop Entry]
Type=Application
Name={name}
Comment={comment}
Exec=\"{bin}\" {args} %u
Icon={icon}
Terminal=false
Categories={categories}
MimeType={mime};
StartupWMClass={wm_class}
",
        name = app.name(),
        comment = app.comment(),
        args = app.run_args(),
        icon = app.icon_name(),
        categories = match app {
            App::Client => "Game;",
            App::Studio => "Development;",
        },
        mime = scheme_mime(app),
        // Wine names its X11 windows after the exe, lowercased
        wm_class = app.exe_name().to_ascii_lowercase(),
    ))
}

fn scheme_mime(app: App) -> String {
    format!("x-scheme-handler/{}", app.scheme())
}

// the exe carries its own icon, so Studio gets the right one and we skip a download
fn install_icon(paths: &Paths, app: App) -> Result<()> {
    let exe = fs::read(paths.app_exe(app))?;
    let png = largest_png(&exe_icon(&exe)?)?;
    let icon_file = paths.icon_file(app);
    fs::create_dir_all(&paths.icons_dir)?;
    fs::write(&icon_file, png)?;
    Ok(())
}

/// Rebuilds the first icon group of a 64 bit PE file as a standalone `.ico`.
pub fn exe_icon(exe: &[u8]) -> Result<Vec<u8>> {
    let pe = PeFile::from_bytes(exe).context("not a 64 bit Windows exe")?;
    let resources = pe.resources().context("exe has no resources")?;
    let (_, group) = resources
        .icons()
        .next()
        .context("exe has no icon")?
        .context("exe icon is broken")?;
    let mut ico = Vec::new();
    group.write(&mut ico)?;
    Ok(ico)
}

pub fn largest_png(ico: &[u8]) -> Result<Vec<u8>> {
    let dir = ico::IconDir::read(Cursor::new(ico)).context("not a valid .ico")?;
    let biggest = dir
        .entries()
        .iter()
        .max_by_key(|e| e.width())
        .context(".ico has no images")?;
    let mut png = Vec::new();
    biggest.decode()?.write_png(&mut png)?;
    Ok(png)
}

fn run_quietly(cmd: &mut Command) {
    let name = cmd.get_program().to_string_lossy().into_owned();
    match cmd.output() {
        Ok(out) if out.status.success() => {}
        Ok(out) => eprintln!(
            "warning: {name} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => eprintln!("warning: couldn't run {name}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIN: &str = "/home/u/.local/bin/vortexlinux";

    #[test]
    fn client_entry() {
        let e = entry(App::Client, Path::new(BIN)).unwrap();
        assert!(e.contains("Name=Vortex\n"));
        assert!(e.contains("Exec=\"/home/u/.local/bin/vortexlinux\" run %u\n"));
        assert!(e.contains("Icon=vortexlinux\n"));
        assert!(e.contains("MimeType=x-scheme-handler/vortex;\n"));
        assert!(e.contains("StartupWMClass=vortex.exe\n"));
    }

    #[test]
    fn studio_entry() {
        let e = entry(App::Studio, Path::new(BIN)).unwrap();
        assert!(e.contains("Name=Vortex Studio\n"));
        assert!(e.contains("Exec=\"/home/u/.local/bin/vortexlinux\" run --studio %u\n"));
        assert!(e.contains("Icon=vortexlinux-studio\n"));
        assert!(e.contains("MimeType=x-scheme-handler/vortex-studio;\n"));
        assert!(e.contains("StartupWMClass=vortexstudio.exe\n"));
    }

    #[test]
    fn entry_allows_unicode_paths() {
        assert!(entry(App::Client, Path::new("/home/ü/Masaüstü/vortexlinux")).is_ok());
    }

    #[test]
    fn entry_refuses_paths_it_cant_quote() {
        assert!(entry(App::Client, Path::new("/home/$USER/bin")).is_err());
        assert!(entry(App::Client, Path::new("/home/a\"b/bin")).is_err());
    }

    #[test]
    fn non_exe_has_no_icon() {
        assert!(exe_icon(b"definitely not a PE file").is_err());
    }

    #[test]
    fn picks_the_biggest_icon() {
        let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
        for size in [16u32, 64, 32] {
            let img =
                ico::IconImage::from_rgba_data(size, size, vec![0; (size * size * 4) as usize]);
            dir.add_entry(ico::IconDirEntry::encode(&img).unwrap());
        }
        let mut bytes = Vec::new();
        dir.write(&mut bytes).unwrap();

        let png = largest_png(&bytes).unwrap();
        let img = ico::IconImage::read_png(Cursor::new(png)).unwrap();
        assert_eq!(img.width(), 64);
    }
}
