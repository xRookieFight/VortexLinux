use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::app::App;
use crate::paths::Paths;

// the zips are ~60 MB today, this is just a sanity cap against a runaway download
const MAX_ZIP_BYTES: u64 = 1 << 30;

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    UpToDate(String),
    Updated { from: Option<String>, to: String },
}

pub fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("vortexlinux/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .build()
        .into()
}

/// Brings the app's exe up to the version the API advertises. With `force` it
/// redownloads even when our recorded version already matches.
pub fn update(paths: &Paths, app: App, force: bool) -> Result<Outcome> {
    let agent = http_agent();
    let remote = remote_version(&agent, app)?;
    let local = installed_version(paths, app);

    if !force && local.as_deref() == Some(remote.as_str()) && paths.app_exe(app).is_file() {
        return Ok(Outcome::UpToDate(remote));
    }

    install_version(&agent, paths, app, &remote)?;
    Ok(Outcome::Updated {
        from: local,
        to: remote,
    })
}

pub fn remote_version(agent: &ureq::Agent, app: App) -> Result<String> {
    let body = agent
        .get(app.version_url())
        .call()
        .and_then(|mut r| r.body_mut().read_to_string())
        .with_context(|| format!("couldn't reach the {} version API", app.name()))?;
    parse_version(&body)
}

pub fn installed_version(paths: &Paths, app: App) -> Option<String> {
    let v = fs::read_to_string(paths.version_file(app)).ok()?;
    let v = v.trim();
    is_valid_version(v).then(|| v.to_owned())
}

pub fn parse_version(json: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct VersionReply {
        version: String,
    }

    let parsed: VersionReply =
        serde_json::from_str(json).context("unexpected reply from the version API")?;
    // this string ends up in a URL and a file path, so be strict about it
    if !is_valid_version(&parsed.version) {
        bail!("version API returned a weird version: {:?}", parsed.version);
    }
    Ok(parsed.version)
}

fn is_valid_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 6 && p.bytes().all(|b| b.is_ascii_digit()))
}

fn install_version(agent: &ureq::Agent, paths: &Paths, app: App, version: &str) -> Result<()> {
    let dir = paths.app_dir(app);
    fs::create_dir_all(&dir)?;

    let zip_part = dir.join(format!("{}.part", app.zip_name()));
    let exe_part = dir.join(format!("{}.part", app.exe_name()));
    let result = (|| {
        download(agent, &app.release_url(version), &zip_part, app.name())?;
        extract_exe(File::open(&zip_part)?, app.exe_in_zip(), &exe_part)?;
        fs::rename(&exe_part, paths.app_exe(app))?;
        fs::write(paths.version_file(app), format!("{version}\n"))?;
        Ok(())
    })();

    let _ = fs::remove_file(&zip_part);
    let _ = fs::remove_file(&exe_part);
    result
}

fn download(agent: &ureq::Agent, url: &str, dest: &Path, label: &str) -> Result<()> {
    let resp = agent
        .get(url)
        .call()
        .with_context(|| format!("download failed: {url}"))?;
    let total = resp.body().content_length();
    let mut reader = resp
        .into_body()
        .into_with_config()
        .limit(MAX_ZIP_BYTES)
        .reader();
    let mut out = File::create(dest).with_context(|| format!("can't write {}", dest.display()))?;

    let mut progress = Progress::new(label, total);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).context("download interrupted")?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        progress.advance(n as u64);
    }
    progress.finish();
    out.sync_all()?;
    Ok(())
}

/// Pulls `entry_name` out of the release zip into `dest`. Nothing else in the
/// archive is touched, so paths inside the zip can't escape anywhere.
pub fn extract_exe<R: Read + Seek>(zip: R, entry_name: &str, dest: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(zip).context("release zip is corrupt")?;
    let mut entry = archive
        .by_name(entry_name)
        .with_context(|| format!("release zip has no {entry_name}"))?;
    let mut out = File::create(dest)?;
    io::copy(&mut entry, &mut out).with_context(|| format!("failed to unpack {entry_name}"))?;
    out.sync_all()?;
    Ok(())
}

struct Progress<'a> {
    label: &'a str,
    total: Option<u64>,
    done: u64,
    last_shown: u64,
}

impl<'a> Progress<'a> {
    fn new(label: &'a str, total: Option<u64>) -> Self {
        Self {
            label,
            total,
            done: 0,
            last_shown: 0,
        }
    }

    fn advance(&mut self, n: u64) {
        self.done += n;
        // redraw every ~2 MB, plenty for a terminal and a no-op in a desktop launch
        if self.done - self.last_shown >= 2 << 20 {
            self.last_shown = self.done;
            self.draw();
        }
    }

    fn finish(&mut self) {
        self.draw();
        eprintln!();
    }

    fn draw(&self) {
        let mb = self.done as f64 / (1 << 20) as f64;
        match self.total {
            Some(t) if t > 0 => {
                let pct = self.done * 100 / t;
                eprint!("\rdownloading {}: {mb:.1} MB ({pct}%)", self.label);
            }
            _ => eprint!("\rdownloading {}: {mb:.1} MB", self.label),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zip::write::SimpleFileOptions;

    fn make_zip(entries: &[(&str, &[u8])]) -> Cursor<Vec<u8>> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            w.start_file(*name, SimpleFileOptions::default()).unwrap();
            w.write_all(data).unwrap();
        }
        let mut c = w.finish().unwrap();
        c.set_position(0);
        c
    }

    #[test]
    fn parses_api_reply() {
        assert_eq!(parse_version(r#"{"version":"0.6.3"}"#).unwrap(), "0.6.3");
    }

    #[test]
    fn rejects_versions_that_could_escape() {
        for bad in ["../../x", "0.6", "0.6.3.1", "0.6.a", "", "0..3", "1.2.3/"] {
            let json = format!(r#"{{"version":{bad:?}}}"#);
            assert!(parse_version(&json).is_err(), "{bad} should be rejected");
        }
        assert!(parse_version("not json").is_err());
    }

    #[test]
    fn extracts_only_the_exe() {
        let dir = tempfile::tempdir().unwrap();
        let zip = make_zip(&[
            ("Vortex/", b""),
            ("Vortex/Vortex.exe", b"MZ fake"),
            ("../evil", b"nope"),
        ]);
        let dest = dir.path().join("out.exe");
        extract_exe(zip, App::Client.exe_in_zip(), &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"MZ fake");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn missing_exe_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let zip = make_zip(&[("Other/thing.txt", b"hi")]);
        let dest = dir.path().join("out.exe");
        assert!(extract_exe(zip, App::Client.exe_in_zip(), &dest).is_err());
    }

    #[test]
    fn installed_version_ignores_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path(), dir.path().join("c"), dir.path().join("d"));
        fs::create_dir_all(paths.app_dir(App::Studio)).unwrap();
        assert_eq!(installed_version(&paths, App::Studio), None);
        fs::write(paths.version_file(App::Studio), "0.6.3\n").unwrap();
        assert_eq!(
            installed_version(&paths, App::Studio).as_deref(),
            Some("0.6.3")
        );
        fs::write(paths.version_file(App::Studio), "lol").unwrap();
        assert_eq!(installed_version(&paths, App::Studio), None);
    }
}
