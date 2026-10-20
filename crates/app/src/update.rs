//! Self-update from GitHub Releases: finds a newer release, downloads the
//! file published for this platform and swaps it in place of the installed
//! app. The running app keeps its old code in memory until it is restarted.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::util;

const LATEST: &str = "https://api.github.com/repos/Meus-Prompts/brio-git/releases/latest";
/// Between automatic checks while the app stays open.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
/// After start, so the first check does not compete with loading the tabs.
pub const FIRST_CHECK: Duration = Duration::from_secs(10);
/// The AppImage is ~40 MB; anything far larger is not ours.
const MAX_DOWNLOAD: u64 = 300 * 1024 * 1024;

/// What a check found.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    UpToDate,
    /// The new version is installed; restarting runs it.
    Ready { version: String, launch: PathBuf },
    /// Newer, but it could not be installed here; the release page has it.
    Available { version: String, page: String, reason: String },
}

impl Outcome {
    pub fn message(&self) -> String {
        match self {
            Outcome::UpToDate => format!("Você já está na versão mais recente (v{}).", util::VERSION),
            Outcome::Ready { version, .. } => format!("brio-git v{version} instalada. Reinicie para usar."),
            Outcome::Available { version, reason, .. } => format!("brio-git v{version} disponível, mas {reason}."),
        }
    }
}

/// Only release builds update themselves; `cargo run` and `cargo install` do not.
pub fn enabled() -> bool {
    util::VERSION != "dev"
}

/// Checks GitHub and installs a newer release. Blocking; run it off the UI thread.
pub fn check_and_install() -> Result<Outcome, String> {
    let release = latest()?;
    let version = release.tag_name.trim_start_matches('v').to_owned();
    if !is_newer(&version, util::VERSION) {
        return Ok(Outcome::UpToDate);
    }
    let available = |reason: String| Outcome::Available { version: version.clone(), page: release.html_url.clone(), reason };
    let install = match Install::detect() {
        Ok(i) => i,
        Err(e) => return Ok(available(e)),
    };
    let name = asset_name(&install, &version);
    let Some(asset) = release.assets.iter().find(|a| a.name == name) else {
        return Ok(available(format!("a versão {version} não tem o arquivo {name}")));
    };
    let bytes = download(asset)?;
    match install.replace(&bytes) {
        Ok(()) => Ok(Outcome::Ready { version, launch: install.target().to_path_buf() }),
        Err(e) => Ok(available(format!("não foi possível instalar em {}: {e}", install.target().display()))),
    }
}

/// Starts the installed app (the new version, once replaced) detached from this one.
pub fn relaunch(launch: &Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    #[cfg(target_os = "macos")]
    let mut cmd = if launch.extension().is_some_and(|e| e == "app") {
        let mut c = Command::new("open");
        c.arg("-n").arg(launch);
        c
    } else {
        Command::new(launch)
    };
    #[cfg(not(target_os = "macos"))]
    let mut cmd = Command::new(launch);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(drop)
}

/// Removes what an earlier update left behind (the old Windows `.exe`).
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(with_suffix(&exe, ".old"));
    }
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// `sha256:<hex>`, published by GitHub for each uploaded file.
    digest: Option<String>,
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(300)))
            .user_agent(format!("brio-git/{}", util::VERSION))
            .build()
            .into()
    })
}

fn latest() -> Result<Release, String> {
    let mut resp = agent()
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("não foi possível consultar as versões: {e}"))?;
    let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| format!("resposta inesperada do GitHub: {e}"))
}

fn download(asset: &Asset) -> Result<Vec<u8>, String> {
    let mut resp = agent()
        .get(&asset.browser_download_url)
        .call()
        .map_err(|e| format!("não foi possível baixar {}: {e}", asset.name))?;
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(MAX_DOWNLOAD)
        .read_to_vec()
        .map_err(|e| format!("download de {} interrompido: {e}", asset.name))?;
    if let Some(expected) = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
        let actual: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!("{} veio corrompido (SHA-256 não confere)", asset.name));
        }
    }
    Ok(bytes)
}

/// `0.10.0` > `0.9.3`. Anything that is not `x.y.z` is never newer.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn parse(v: &str) -> Option<(u64, u64, u64)> {
        let mut parts = v.trim_start_matches('v').split('.').map(|p| p.parse::<u64>().ok());
        let version = (parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(version)
    }
    matches!((parse(candidate), parse(current)), (Some(a), Some(b)) if a > b)
}

/// How this copy of the app was installed, which decides the file to download.
#[derive(Debug, Clone, PartialEq)]
enum Install {
    /// Linux AppImage: the `.AppImage` file itself is replaced.
    AppImage(PathBuf),
    /// Linux binary from the `.tar.gz`.
    LinuxBinary(PathBuf),
    /// macOS bundle: the whole `brio-git.app` is replaced.
    MacApp(PathBuf),
    WindowsExe(PathBuf),
}

impl Install {
    fn detect() -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = exe.canonicalize().unwrap_or(exe);
        if cfg!(target_os = "windows") {
            return Ok(Self::WindowsExe(exe));
        }
        if cfg!(target_os = "macos") {
            // brio-git.app/Contents/MacOS/brio-git
            return match exe.ancestors().nth(3) {
                Some(app) if app.extension().is_some_and(|e| e == "app") => Ok(Self::MacApp(app.to_path_buf())),
                _ => Err("no macOS só o brio-git.app se atualiza sozinho".into()),
            };
        }
        if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Err("não há versão pronta para este sistema".into());
        }
        // Set by the AppImage runtime to the file the user opened.
        match std::env::var_os("APPIMAGE").map(PathBuf::from) {
            Some(file) if file.is_file() => Ok(Self::AppImage(file)),
            _ => Ok(Self::LinuxBinary(exe)),
        }
    }

    /// What gets replaced, and what runs the new version.
    fn target(&self) -> &Path {
        match self {
            Self::AppImage(p) | Self::LinuxBinary(p) | Self::MacApp(p) | Self::WindowsExe(p) => p,
        }
    }

    fn replace(&self, download: &[u8]) -> std::io::Result<()> {
        match self {
            Self::AppImage(file) => swap_file(file, download),
            Self::LinuxBinary(file) => swap_file(file, &from_tar_gz(download, "brio-git")?),
            Self::MacApp(app) => swap_app(app, download),
            Self::WindowsExe(exe) => {
                let new = from_zip(download, "brio-git.exe")?;
                // A running .exe cannot be overwritten, but it can be renamed.
                let staged = with_suffix(exe, ".new");
                std::fs::write(&staged, new)?;
                let old = with_suffix(exe, ".old");
                let _ = std::fs::remove_file(&old);
                std::fs::rename(exe, &old)?;
                std::fs::rename(&staged, exe).inspect_err(|_| {
                    let _ = std::fs::rename(&old, exe);
                })
            }
        }
    }
}

fn asset_name(install: &Install, version: &str) -> String {
    let suffix = match install {
        Install::AppImage(_) => "x86_64.AppImage",
        Install::LinuxBinary(_) => "linux-x86_64.tar.gz",
        Install::MacApp(_) => "macos-universal.zip",
        Install::WindowsExe(_) => "windows-x86_64.zip",
    };
    format!("brio-git-{version}-{suffix}")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Writes `bytes` next to `file` and renames it over: the file is either the
/// old or the new one, never half written.
fn swap_file(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let staged = with_suffix(file, ".new");
    std::fs::write(&staged, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&staged, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&staged);
    })
}

fn from_tar_gz(bytes: &[u8], name: &str) -> std::io::Result<Vec<u8>> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.path()?.file_name().is_some_and(|n| n == name) {
            let mut out = Vec::new();
            entry.read_to_end(&mut out)?;
            return Ok(out);
        }
    }
    Err(std::io::Error::other(format!("{name} não está no pacote")))
}

fn from_zip(bytes: &[u8], name: &str) -> std::io::Result<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(std::io::Error::other)?;
    let mut file = archive.by_name(name).map_err(|_| std::io::Error::other(format!("{name} não está no pacote")))?;
    let mut out = Vec::new();
    file.read_to_end(&mut out)?;
    Ok(out)
}

/// Unpacks the new `brio-git.app` beside the installed one, then swaps the two folders.
fn swap_app(app: &Path, zip_bytes: &[u8]) -> std::io::Result<()> {
    let parent = app.parent().ok_or_else(|| std::io::Error::other("pasta do app desconhecida"))?;
    let staging = parent.join(".brio-git-update");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;
    let result = (|| {
        unzip(zip_bytes, &staging)?;
        let fresh = staging.join("brio-git.app");
        if !fresh.join("Contents").is_dir() {
            return Err(std::io::Error::other("brio-git.app não está no pacote"));
        }
        let old = staging.join("old.app");
        std::fs::rename(app, &old)?;
        std::fs::rename(&fresh, app).inspect_err(|_| {
            let _ = std::fs::rename(&old, app);
        })
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Extracts every entry under `dest`, keeping Unix permissions; entries that
/// would land outside `dest` are refused.
fn unzip(bytes: &[u8], dest: &Path) -> std::io::Result<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(std::io::Error::other)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(std::io::Error::other)?;
        let Some(relative) = file.enclosed_name() else {
            return Err(std::io::Error::other("caminho inválido no pacote"));
        };
        let path = dest.join(relative);
        if file.is_dir() {
            std::fs::create_dir_all(&path)?;
            continue;
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut out = std::fs::File::create(&path)?;
        std::io::copy(&mut file, &mut out)?;
        #[cfg(unix)]
        if let Some(mode) = file.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.3.0", "0.2.0"));
        assert!(is_newer("v0.10.0", "0.9.3"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(!is_newer("0.3.0", "dev"));
        assert!(!is_newer("0.3", "0.2.0"));
        assert!(!is_newer("0.3.0-beta", "0.2.0"));
    }

    #[test]
    fn asset_matches_the_release_file_names() {
        let p = PathBuf::from("x");
        assert_eq!(asset_name(&Install::AppImage(p.clone()), "0.3.0"), "brio-git-0.3.0-x86_64.AppImage");
        assert_eq!(asset_name(&Install::LinuxBinary(p.clone()), "0.3.0"), "brio-git-0.3.0-linux-x86_64.tar.gz");
        assert_eq!(asset_name(&Install::MacApp(p.clone()), "0.3.0"), "brio-git-0.3.0-macos-universal.zip");
        assert_eq!(asset_name(&Install::WindowsExe(p), "0.3.0"), "brio-git-0.3.0-windows-x86_64.zip");
    }

    fn tar_gz(name: &str, data: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append_data(&mut header, name, data).unwrap();
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
        for (name, data) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn linux_binary_is_swapped_from_the_tarball() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("brio-git");
        std::fs::write(&bin, b"old").unwrap();
        Install::LinuxBinary(bin.clone()).replace(&tar_gz("brio-git", b"new")).unwrap();
        assert_eq!(std::fs::read(&bin).unwrap(), b"new");
        assert!(!with_suffix(&bin, ".new").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&bin).unwrap().permissions().mode() & 0o777, 0o755);
        }
    }

    #[test]
    fn windows_exe_is_renamed_aside() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("brio-git.exe");
        std::fs::write(&exe, b"old").unwrap();
        Install::WindowsExe(exe.clone()).replace(&zip_of(&[("brio-git.exe", b"new")])).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(std::fs::read(with_suffix(&exe, ".old")).unwrap(), b"old");
    }

    #[test]
    fn mac_bundle_is_replaced_whole() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("brio-git.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/MacOS/brio-git"), b"old").unwrap();
        std::fs::write(app.join("Contents/stale"), b"x").unwrap();
        let zip = zip_of(&[
            ("brio-git.app/Contents/MacOS/brio-git", b"new"),
            ("brio-git.app/Contents/Info.plist", b"plist"),
        ]);
        Install::MacApp(app.clone()).replace(&zip).unwrap();
        assert_eq!(std::fs::read(app.join("Contents/MacOS/brio-git")).unwrap(), b"new");
        assert!(!app.join("Contents/stale").exists());
        assert!(!dir.path().join(".brio-git-update").exists());
    }

    /// Talks to GitHub: `cargo test -p brio-git -- --ignored published_release`.
    #[test]
    #[ignore]
    fn published_release_downloads_and_verifies() {
        let release = latest().unwrap();
        let version = release.tag_name.trim_start_matches('v');
        let name = asset_name(&Install::LinuxBinary(PathBuf::new()), version);
        let asset = release.assets.iter().find(|a| a.name == name).unwrap();
        assert!(asset.digest.as_deref().is_some_and(|d| d.starts_with("sha256:")), "GitHub publishes the digest");
        let binary = from_tar_gz(&download(asset).unwrap(), "brio-git").unwrap();
        assert!(binary.starts_with(b"\x7fELF"));
    }

    #[test]
    fn a_package_without_the_app_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("brio-git");
        std::fs::write(&bin, b"old").unwrap();
        assert!(Install::LinuxBinary(bin.clone()).replace(&tar_gz("other", b"new")).is_err());
        assert_eq!(std::fs::read(&bin).unwrap(), b"old");

        let app = dir.path().join("brio-git.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        assert!(Install::MacApp(app.clone()).replace(&zip_of(&[("readme", b"x")])).is_err());
        assert!(app.join("Contents").is_dir());
    }
}
