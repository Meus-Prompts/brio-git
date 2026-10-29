//! Repositórios dentro do WSL abertos pelo `.exe` do Windows
//! (`\\wsl$\Ubuntu\home\ana\repo` ou `\\wsl.localhost\Ubuntu\...`).
//! O git roda dentro da distro (`wsl.exe -d <distro> --cd <caminho>`), usando
//! o git, as credenciais e as chaves SSH do próprio WSL.

use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslPath {
    pub distro: String,
    /// Caminho dentro da distro (`/home/ana/repo`).
    pub linux: String,
}

/// Reconhece caminhos UNC do WSL (com `\` ou `/`, prefixo `\\?\UNC\` opcional).
pub fn parse(path: &Path) -> Option<WslPath> {
    let s = path.to_string_lossy().replace('/', "\\");
    let s = s.strip_prefix(r"\\?\UNC\").map(|r| format!(r"\\{r}")).unwrap_or(s);
    let rest = s.strip_prefix(r"\\wsl$\").or_else(|| s.strip_prefix(r"\\wsl.localhost\"))?;
    let (distro, tail) = rest.split_once('\\').unwrap_or((rest, ""));
    if distro.is_empty() {
        return None;
    }
    let linux = format!("/{}", tail.trim_matches('\\').replace('\\', "/"));
    Some(WslPath { distro: distro.to_string(), linux: if linux == "/" { linux } else { linux.trim_end_matches('/').to_string() } })
}

/// Converte um caminho do Windows para o que o git dentro do WSL enxerga:
/// UNC do WSL → caminho da distro; `C:\x\y` → `/mnt/c/x/y`.
pub fn to_linux(path: &Path) -> String {
    if let Some(w) = parse(path) {
        return w.linux;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    let b = s.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        return format!("/mnt/{}{}", (b[0] as char).to_ascii_lowercase(), &s[2..]);
    }
    s
}

/// Caminho de arquivo para scripts de shell executados pelo git: no WSL vira
/// caminho Linux; no Git for Windows, `/` evita escapes de `\` no `sh`.
pub fn for_git_shell(repo: &Path, file: &Path) -> String {
    if parse(repo).is_some() { to_linux(file) } else { file.to_string_lossy().replace('\\', "/") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_wsl_unc_paths() {
        let p = parse(&PathBuf::from(r"\\wsl$\Ubuntu-24.04\home\ana\repo\")).unwrap();
        assert_eq!((p.distro.as_str(), p.linux.as_str()), ("Ubuntu-24.04", "/home/ana/repo"));
        let p = parse(&PathBuf::from("//wsl.localhost/Debian/srv/x")).unwrap();
        assert_eq!((p.distro.as_str(), p.linux.as_str()), ("Debian", "/srv/x"));
        let p = parse(&PathBuf::from(r"\\?\UNC\wsl.localhost\Ubuntu\home")).unwrap();
        assert_eq!(p.linux, "/home");
        assert_eq!(parse(&PathBuf::from(r"\\wsl$\Ubuntu")).unwrap().linux, "/");
        assert!(parse(&PathBuf::from(r"C:\Users\ana")).is_none());
        assert!(parse(&PathBuf::from("/home/ana")).is_none());
    }

    #[test]
    fn converts_windows_paths_for_git_in_wsl() {
        assert_eq!(to_linux(&PathBuf::from(r"C:\Users\Ana\AppData\Local\Temp\todo")), "/mnt/c/Users/Ana/AppData/Local/Temp/todo");
        assert_eq!(to_linux(&PathBuf::from(r"\\wsl$\Ubuntu\home\ana\repo\.git\x")), "/home/ana/repo/.git/x");
        let repo = PathBuf::from(r"\\wsl$\Ubuntu\home\ana\repo");
        assert_eq!(for_git_shell(&repo, &PathBuf::from(r"D:\tmp\t")), "/mnt/d/tmp/t");
        assert_eq!(for_git_shell(&PathBuf::from(r"C:\code\repo"), &PathBuf::from(r"C:\tmp\t")), "C:/tmp/t");
    }
}
