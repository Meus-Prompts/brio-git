//! Atualização automática pelas Releases do GitHub (tags `vX.Y.Z`). Encontra
//! a release mais nova, baixa o arquivo desta plataforma, confere o SHA-256 e
//! troca o app instalado. O app em execução mantém o código antigo em memória
//! até ser reiniciado.
//!
//! Os nomes dos arquivos são os mesmos usados pelo brio-git em iced, então
//! quem tem a versão antiga instalada recebe esta pela atualização automática.
//! Releases de outros canais (`egui-vX.Y.Z`) são ignoradas.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const REPO: &str = "Meus-Prompts/brio-git";
/// Prefixo das tags de release.
const TAG_PREFIX: &str = "v";
/// Versão deste build, definida pelo pipeline de release; `dev` em builds locais.
pub const VERSION: &str = match option_env!("BRIO_VERSION") {
    Some(v) => v,
    None => "dev",
};
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
pub const FIRST_CHECK: Duration = Duration::from_secs(10);
const MAX_DOWNLOAD: u64 = 300 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    UpToDate,
    /// A versão nova está instalada; reiniciar passa a usá-la.
    Ready { version: String, launch: PathBuf },
    /// Há versão nova, mas não deu para instalar aqui; a página da release tem o arquivo.
    Available { version: String, page: String, reason: String },
}

impl Outcome {
    pub fn message(&self) -> String {
        match self {
            Outcome::UpToDate => format!("Você já está na versão mais recente (v{VERSION})."),
            Outcome::Ready { version, .. } => format!("brio-git v{version} instalada. Reinicie para usar."),
            Outcome::Available { version, reason, .. } => format!("brio-git v{version} disponível, mas {reason}."),
        }
    }
}

/// Só builds de release se atualizam; `cargo run` não.
pub fn enabled() -> bool {
    VERSION != "dev"
}

/// Consulta o GitHub e instala uma release mais nova. Bloqueante: rode fora da UI.
pub fn check_and_install() -> Result<Outcome, String> {
    let Some(release) = newest(&releases()?) else { return Ok(Outcome::UpToDate) };
    let version = release.tag_name.trim_start_matches(TAG_PREFIX).to_owned();
    if !is_newer(&version, VERSION) {
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

/// Abre o app instalado (já a versão nova) desacoplado deste processo.
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

/// Remove o que uma atualização anterior deixou (o `.exe` antigo no Windows).
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(with_suffix(&exe, ".old"));
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    /// `sha256:<hex>`, publicado pelo GitHub para cada arquivo enviado.
    digest: Option<String>,
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(300))).user_agent(format!("brio-git/{VERSION}")).build().into())
}

fn releases() -> Result<Vec<Release>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=50");
    let mut resp = agent()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("não foi possível consultar as versões: {e}"))?;
    let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| format!("resposta inesperada do GitHub: {e}"))
}

/// Release publicada mais nova `vX.Y.Z` (ignora rascunhos e outros canais).
fn newest(list: &[Release]) -> Option<Release> {
    let version = |r: &Release| r.tag_name.strip_prefix(TAG_PREFIX).and_then(parse);
    list.iter().filter(|r| !r.draft && version(r).is_some()).max_by_key(|r| version(r)).cloned()
}

fn download(asset: &Asset) -> Result<Vec<u8>, String> {
    let mut resp = agent().get(&asset.browser_download_url).call().map_err(|e| format!("não foi possível baixar {}: {e}", asset.name))?;
    let bytes = resp.body_mut().with_config().limit(MAX_DOWNLOAD).read_to_vec().map_err(|e| format!("download de {} interrompido: {e}", asset.name))?;
    if let Some(expected) = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
        let actual: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!("{} veio corrompido (SHA-256 não confere)", asset.name));
        }
    }
    Ok(bytes)
}

fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.trim_start_matches('v').split('.').map(|p| p.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// `0.10.0` > `0.9.3`. O que não for `x.y.z` nunca é mais novo.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse(candidate), parse(current)), (Some(a), Some(b)) if a > b)
}

/// Como esta cópia foi instalada, o que decide o arquivo a baixar.
#[derive(Debug, Clone, PartialEq)]
enum Install {
    /// AppImage: o próprio arquivo `.AppImage` é trocado.
    AppImage(PathBuf),
    /// Binário Linux do `.tar.gz` (também o caminho no WSL).
    LinuxBinary(PathBuf),
    /// macOS: o `brio-git.app` inteiro é trocado.
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
        // Definida pelo runtime do AppImage com o arquivo que o usuário abriu.
        match std::env::var_os("APPIMAGE").map(PathBuf::from) {
            Some(file) if file.is_file() => Ok(Self::AppImage(file)),
            _ => Ok(Self::LinuxBinary(exe)),
        }
    }

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
                // Um .exe em execução não pode ser sobrescrito, mas pode ser renomeado.
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

/// Grava ao lado e renomeia por cima: o arquivo é o antigo ou o novo, nunca pela metade.
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

/// Descompacta o `brio-git.app` novo ao lado do instalado e troca as pastas.
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

/// Extrai tudo sob `dest`, mantendo permissões Unix; entradas que cairiam
/// fora de `dest` são recusadas.
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

    fn rel(tag: &str, draft: bool) -> Release {
        Release { tag_name: tag.into(), html_url: String::new(), draft, assets: vec![] }
    }

    #[test]
    fn picks_the_newest_v_release_only() {
        let list = vec![rel("egui-v9.9.9", false), rel("v0.5.1", false), rel("v0.10.0", false), rel("v1.0.0", true), rel("vX", false)];
        assert_eq!(newest(&list).unwrap().tag_name, "v0.10.0", "ignora outros canais, rascunhos e tags inválidas");
        assert!(newest(&[rel("egui-v1.0.0", false)]).is_none());
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.10.0", "0.9.3"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.3.0", "dev"));
        assert!(!is_newer("0.3.0-beta", "0.2.0"));
    }

    /// Os mesmos nomes que o updater do brio-git em iced procura: quem tem a
    /// versão antiga recebe esta.
    #[test]
    fn asset_names_match_what_installed_apps_look_for() {
        let p = PathBuf::from("x");
        assert_eq!(asset_name(&Install::AppImage(p.clone()), "0.6.0"), "brio-git-0.6.0-x86_64.AppImage");
        assert_eq!(asset_name(&Install::LinuxBinary(p.clone()), "0.6.0"), "brio-git-0.6.0-linux-x86_64.tar.gz");
        assert_eq!(asset_name(&Install::MacApp(p.clone()), "0.6.0"), "brio-git-0.6.0-macos-universal.zip");
        assert_eq!(asset_name(&Install::WindowsExe(p), "0.6.0"), "brio-git-0.6.0-windows-x86_64.zip");
    }

    #[test]
    fn mac_bundle_is_replaced_whole() {
        let dir = tmp("mac");
        let app = dir.join("brio-git.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/MacOS/brio-git"), b"old").unwrap();
        std::fs::write(app.join("Contents/stale"), b"x").unwrap();
        let zip = zip_of(&[("brio-git.app/Contents/MacOS/brio-git", b"new"), ("brio-git.app/Contents/Info.plist", b"plist")]);
        Install::MacApp(app.clone()).replace(&zip).unwrap();
        assert_eq!(std::fs::read(app.join("Contents/MacOS/brio-git")).unwrap(), b"new");
        assert!(!app.join("Contents/stale").exists());
        assert!(!dir.join(".brio-git-update").exists());
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

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("brio-git-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn linux_binary_is_swapped_from_the_tarball() {
        let bin = tmp("linux").join("brio-git");
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
        let exe = tmp("win").join("brio-git.exe");
        std::fs::write(&exe, b"old").unwrap();
        Install::WindowsExe(exe.clone()).replace(&zip_of(&[("brio-git.exe", b"new")])).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(std::fs::read(with_suffix(&exe, ".old")).unwrap(), b"old");
    }

    #[test]
    fn a_package_without_the_app_changes_nothing() {
        let bin = tmp("bad").join("brio-git");
        std::fs::write(&bin, b"old").unwrap();
        assert!(Install::LinuxBinary(bin.clone()).replace(&tar_gz("other", b"new")).is_err());
        assert_eq!(std::fs::read(&bin).unwrap(), b"old");
    }
}
