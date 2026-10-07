use anyhow::{Result, bail};

const MAX_URL_LEN: usize = 2048;

/// The Windows programs we know how to install and run. They ship separately,
/// each with its own version API, CDN folder and URL scheme, but share the prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    Client,
    Studio,
}

impl App {
    pub const ALL: [App; 2] = [App::Client, App::Studio];

    pub fn from_flag(studio: bool) -> Self {
        if studio { Self::Studio } else { Self::Client }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Client => "Vortex",
            Self::Studio => "Vortex Studio",
        }
    }

    pub fn comment(self) -> &'static str {
        match self {
            Self::Client => "Play games on Vortex",
            Self::Studio => "Build games for Vortex",
        }
    }

    // same word in both the API path and the CDN folder, handy
    fn channel(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Studio => "studio",
        }
    }

    pub fn version_url(self) -> String {
        format!("https://playvortex.io/api/{}-version", self.channel())
    }

    /// `version` must already be validated, it goes into the URL as is.
    pub fn release_url(self, version: &str) -> String {
        format!(
            "https://cdn.playvortex.io/releases/{}/{version}/{}",
            self.channel(),
            self.zip_name()
        )
    }

    pub fn zip_name(self) -> &'static str {
        match self {
            Self::Client => "Vortex-Windows.zip",
            Self::Studio => "VortexStudio-Windows.zip",
        }
    }

    pub fn exe_name(self) -> &'static str {
        match self {
            Self::Client => "Vortex.exe",
            Self::Studio => "VortexStudio.exe",
        }
    }

    pub fn exe_in_zip(self) -> &'static str {
        match self {
            Self::Client => "Vortex/Vortex.exe",
            Self::Studio => "VortexStudio/VortexStudio.exe",
        }
    }

    /// Folder under the data dir. The client's is called `game` because it came first.
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Client => "game",
            Self::Studio => "studio",
        }
    }

    pub fn scheme(self) -> &'static str {
        match self {
            Self::Client => "vortex",
            Self::Studio => "vortex-studio",
        }
    }

    pub fn desktop_id(self) -> &'static str {
        match self {
            Self::Client => "vortexlinux.desktop",
            Self::Studio => "vortexlinux-studio.desktop",
        }
    }

    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Client => "vortexlinux",
            Self::Studio => "vortexlinux-studio",
        }
    }

    /// What goes after our binary in the desktop entry's Exec line.
    pub fn run_args(self) -> &'static str {
        match self {
            Self::Client => "run",
            Self::Studio => "run --studio",
        }
    }

    /// Only lets links of this app's own scheme through, since whatever the
    /// browser hands us goes straight onto Wine's command line.
    pub fn validate_url(self, url: &str) -> Result<()> {
        let scheme = self.scheme();
        if url.len() > MAX_URL_LEN {
            bail!("{scheme} link is too long ({} bytes)", url.len());
        }
        let prefix_ok = url
            .split_once("://")
            .is_some_and(|(s, _)| s.eq_ignore_ascii_case(scheme));
        if !prefix_ok {
            bail!("not a {scheme}:// link: {url:?}");
        }
        if url.chars().any(char::is_control) {
            bail!("{scheme} link contains control characters");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_follow_the_cdn_layout() {
        assert_eq!(
            App::Client.release_url("0.6.3"),
            "https://cdn.playvortex.io/releases/client/0.6.3/Vortex-Windows.zip"
        );
        assert_eq!(
            App::Studio.release_url("0.6.1"),
            "https://cdn.playvortex.io/releases/studio/0.6.1/VortexStudio-Windows.zip"
        );
        assert_eq!(
            App::Studio.version_url(),
            "https://playvortex.io/api/studio-version"
        );
    }

    #[test]
    fn client_links() {
        assert!(App::Client.validate_url("vortex://event/abc").is_ok());
        assert!(App::Client.validate_url("VORTEX://event").is_ok());
        assert!(App::Client.validate_url("https://evil.example").is_err());
        assert!(App::Client.validate_url("vortex:/x").is_err());
        assert!(App::Client.validate_url("vortex://a\nb").is_err());
        assert!(App::Client.validate_url("vörtex://x").is_err());
        let long = format!("vortex://{}", "a".repeat(MAX_URL_LEN));
        assert!(App::Client.validate_url(&long).is_err());
    }

    #[test]
    fn each_app_only_takes_its_own_scheme() {
        assert!(App::Studio.validate_url("vortex-studio://auth?x=1").is_ok());
        assert!(App::Studio.validate_url("vortex://event").is_err());
        assert!(App::Client.validate_url("vortex-studio://auth").is_err());
    }
}
