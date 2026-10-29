//! Gitflow com comandos git puros, compatível com as chaves de config do
//! git-flow AVH / nodegit-flow (`gitflow.branch.*`, `gitflow.prefix.*`).

use std::path::Path;

use anyhow::{Result, anyhow};

use super::cli::git;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitflowConfig {
    pub master: String,
    pub develop: String,
    pub feature: String,
    pub release: String,
    pub hotfix: String,
    pub version_tag: String,
}

impl Default for GitflowConfig {
    fn default() -> Self {
        Self {
            master: "main".into(),
            develop: "develop".into(),
            feature: "feature/".into(),
            release: "release/".into(),
            hotfix: "hotfix/".into(),
            version_tag: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowKind {
    Feature,
    Release,
    Hotfix,
}

impl FlowKind {
    pub const ALL: [FlowKind; 3] = [FlowKind::Feature, FlowKind::Release, FlowKind::Hotfix];

    pub fn label(self) -> &'static str {
        match self {
            FlowKind::Feature => "Feature",
            FlowKind::Release => "Release",
            FlowKind::Hotfix => "Hotfix",
        }
    }

    pub fn prefix(self, c: &GitflowConfig) -> &str {
        match self {
            FlowKind::Feature => &c.feature,
            FlowKind::Release => &c.release,
            FlowKind::Hotfix => &c.hotfix,
        }
    }

    /// Branch de onde o fluxo parte.
    pub fn base(self, c: &GitflowConfig) -> &str {
        match self {
            FlowKind::Hotfix => &c.master,
            _ => &c.develop,
        }
    }

    /// Release e hotfix geram tag e voltam para master + develop.
    pub fn tags(self) -> bool {
        self != FlowKind::Feature
    }
}

/// Lê a configuração; `None` quando o repositório não foi inicializado.
pub fn read(repo: &git2::Repository) -> Option<GitflowConfig> {
    let cfg = repo.config().ok()?;
    let get = |k: &str| cfg.get_string(k).ok();
    Some(GitflowConfig {
        master: get("gitflow.branch.master")?,
        develop: get("gitflow.branch.develop")?,
        feature: get("gitflow.prefix.feature").unwrap_or_else(|| "feature/".into()),
        release: get("gitflow.prefix.release").unwrap_or_else(|| "release/".into()),
        hotfix: get("gitflow.prefix.hotfix").unwrap_or_else(|| "hotfix/".into()),
        version_tag: get("gitflow.prefix.versiontag").unwrap_or_default(),
    })
}

fn branch_exists(repo: &Path, name: &str) -> bool {
    git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{name}")]).is_ok()
}

pub fn init(repo: &Path, c: &GitflowConfig) -> Result<String> {
    if c.master.trim().is_empty() || c.develop.trim().is_empty() || c.master == c.develop {
        return Err(anyhow!("master e develop precisam ser branches diferentes"));
    }
    if !branch_exists(repo, &c.master) {
        return Err(anyhow!("o branch {} não existe", c.master));
    }
    for (k, v) in [
        ("gitflow.branch.master", &c.master),
        ("gitflow.branch.develop", &c.develop),
        ("gitflow.prefix.feature", &c.feature),
        ("gitflow.prefix.release", &c.release),
        ("gitflow.prefix.hotfix", &c.hotfix),
        ("gitflow.prefix.versiontag", &c.version_tag),
    ] {
        git(repo, &["config", k, v])?;
    }
    if !branch_exists(repo, &c.develop) {
        git(repo, &["branch", &c.develop, &c.master])?;
    }
    Ok("Gitflow inicializado".into())
}

pub fn start(repo: &Path, c: &GitflowConfig, kind: FlowKind, name: &str) -> Result<String> {
    let branch = format!("{}{}", kind.prefix(c), name.trim());
    git(repo, &["checkout", "-b", &branch, kind.base(c)])?;
    Ok(format!("Started {}: '{}'", kind.label().to_lowercase(), name.trim()))
}

pub struct Finish<'a> {
    pub kind: FlowKind,
    pub name: &'a str,
    pub tag_message: &'a str,
    pub keep_branch: bool,
}

pub fn finish(repo: &Path, c: &GitflowConfig, f: &Finish) -> Result<String> {
    let branch = format!("{}{}", f.kind.prefix(c), f.name);
    if !branch_exists(repo, &branch) {
        return Err(anyhow!("o branch {branch} não existe"));
    }
    let msg = |target: &str| format!("Merge branch '{branch}' into {target}");
    if f.kind.tags() {
        git(repo, &["checkout", &c.master])?;
        git(repo, &["merge", "--no-ff", "-m", &msg(&c.master), &branch])?;
        let tag = format!("{}{}", c.version_tag, f.name);
        let tag_msg = if f.tag_message.trim().is_empty() { tag.clone() } else { f.tag_message.to_string() };
        git(repo, &["tag", "-a", &tag, "-m", &tag_msg])?;
    }
    git(repo, &["checkout", &c.develop])?;
    // Release/hotfix: o develop recebe o branch (que já está no master).
    git(repo, &["merge", "--no-ff", "-m", &msg(&c.develop), &branch])?;
    if !f.keep_branch {
        git(repo, &["branch", "-d", &branch])?;
    }
    Ok(format!("Finished {}: '{}'", f.kind.label().to_lowercase(), f.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) -> String {
        git(dir, args).unwrap().trim().to_string()
    }

    fn repo() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("brio-git-flow-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        sh(&dir, &["init", "-q", "-b", "main"]);
        sh(&dir, &["config", "user.name", "T"]);
        sh(&dir, &["config", "user.email", "t@t"]);
        sh(&dir, &["config", "commit.gpgsign", "false"]);
        sh(&dir, &["config", "tag.gpgsign", "false"]);
        std::fs::write(dir.join("a"), "a\n").unwrap();
        sh(&dir, &["add", "-A"]);
        sh(&dir, &["commit", "-qm", "init"]);
        dir
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    fn commit(dir: &Path, file: &str) {
        std::fs::write(dir.join(file), file).unwrap();
        sh(dir, &["add", "-A"]);
        sh(dir, &["commit", "-qm", file]);
    }

    #[test]
    fn feature_and_release_lifecycle() {
        let dir = repo();
        let c = GitflowConfig { version_tag: "v".into(), ..Default::default() };
        init(&dir, &c).unwrap();
        assert_eq!(read(&git2::Repository::open(&dir).unwrap()), Some(c.clone()));

        start(&dir, &c, FlowKind::Feature, "login").unwrap();
        assert_eq!(sh(&dir, &["branch", "--show-current"]), "feature/login");
        commit(&dir, "login.txt");
        finish(&dir, &c, &Finish { kind: FlowKind::Feature, name: "login", tag_message: "", keep_branch: false }).unwrap();
        assert_eq!(sh(&dir, &["branch", "--show-current"]), "develop");
        assert!(dir.join("login.txt").exists());
        assert!(sh(&dir, &["branch", "--list", "feature/login"]).is_empty(), "feature apagada");

        start(&dir, &c, FlowKind::Release, "1.0").unwrap();
        commit(&dir, "version.txt");
        finish(&dir, &c, &Finish { kind: FlowKind::Release, name: "1.0", tag_message: "Release 1.0", keep_branch: false }).unwrap();
        assert_eq!(sh(&dir, &["tag", "--list"]), "v1.0");
        // A tag fica no merge do master; o develop recebe o branch de release.
        sh(&dir, &["merge-base", "--is-ancestor", "v1.0", "main"]);
        assert_eq!(sh(&dir, &["show", "develop:version.txt"]), "version.txt");
        assert_eq!(sh(&dir, &["log", "-1", "--format=%s", "main"]), "Merge branch 'release/1.0' into main");
    }

    #[test]
    fn hotfix_branches_from_master() {
        let dir = repo();
        let c = GitflowConfig::default();
        init(&dir, &c).unwrap();
        sh(&dir, &["checkout", "-q", "develop"]);
        commit(&dir, "dev-only.txt");
        start(&dir, &c, FlowKind::Hotfix, "crash").unwrap();
        assert!(!dir.join("dev-only.txt").exists(), "hotfix parte do master, não do develop");
    }
}
