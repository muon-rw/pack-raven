use std::path::Path;
use std::process::Stdio;

use anyhow::Result;
use tokio::io::BufReader;
use tokio::process::Command;

use crate::credentials::{self, Credential};
use crate::git;
use crate::jobs::Log;

const TOKEN_ENV: &str = "PACKRAVEN_GITHUB_TOKEN";
// We hand git the token through the environment, so it never shows up in the process list or in git config.
const GITHUB_HELPER: &str = r#"!f() { test "$1" = get && printf 'username=x-access-token\npassword=%s\n' "$PACKRAVEN_GITHUB_TOKEN"; }; f"#;

pub struct RemoteAuth {
    github_token: Option<String>,
}

impl RemoteAuth {
    pub fn load() -> Self {
        Self { github_token: credentials::get(Credential::Github) }
    }

    // The empty helper clears the list for github.com only, so a stale Git Credential Manager login
    // can't win over the token, while other hosts keep the helpers you configured.
    fn config_args(&self) -> Vec<String> {
        match self.github_token {
            Some(_) => ["-c", "credential.https://github.com.helper=", "-c", &format!("credential.https://github.com.helper={GITHUB_HELPER}")].map(String::from).to_vec(),
            None => Vec::new(),
        }
    }

    fn env(&self) -> Option<(&'static str, &str)> {
        self.github_token.as_deref().map(|token| (TOKEN_ENV, token))
    }
}

#[derive(Debug, PartialEq)]
enum RemoteKind {
    GithubHttps,
    OtherHttps,
    Ssh,
}

fn remote_kind(url: &str) -> RemoteKind {
    let url = url.trim().to_lowercase();
    if url.starts_with("https://github.com/") || url.starts_with("https://www.github.com/") {
        RemoteKind::GithubHttps
    } else if url.starts_with("https://") || url.starts_with("http://") {
        RemoteKind::OtherHttps
    } else {
        RemoteKind::Ssh
    }
}

fn remote_url(dir: &Path) -> Option<String> {
    let branch = git::branch(dir).unwrap_or_default();
    let remote = git::run(dir, &["config", "--get", &format!("branch.{branch}.remote")]).map(|r| r.trim().to_string()).unwrap_or_else(|_| "origin".into());
    git::run(dir, &["remote", "get-url", &remote]).ok().map(|u| u.trim().to_string())
}

#[derive(Debug, PartialEq)]
enum Failure {
    SignIn,
    Rejected,
    Unreachable,
}

fn classify(stderr: &str) -> Option<Failure> {
    let text = stderr.to_lowercase();
    let any = |needles: &[&str]| needles.iter().any(|n| text.contains(n));
    // GitHub answers "Repository not found" for a private repository you aren't signed in to.
    if any(&["authentication failed", "could not read username", "could not read password", "terminal prompts disabled", "invalid username or password", "permission denied (publickey", "host key verification failed", "repository not found", "returned error: 401", "returned error: 403"])
        || (text.contains("permission to") && text.contains("denied"))
    {
        Some(Failure::SignIn)
    } else if any(&["[rejected]", "non-fast-forward", "updates were rejected", "fetch first"]) {
        Some(Failure::Rejected)
    } else if any(&["could not resolve host", "failed to connect", "connection timed out", "connection refused", "network is unreachable", "could not read from remote repository"]) {
        Some(Failure::Unreachable)
    } else {
        None
    }
}

fn explanation(failure: &Failure, kind: &RemoteKind, has_token: bool) -> &'static str {
    match (failure, kind, has_token) {
        (Failure::SignIn, RemoteKind::GithubHttps, true) => {
            "GitHub turned down your token. Check that it hasn't expired and has Contents: Read and write access to this repository, then replace it in App settings."
        }
        (Failure::SignIn, RemoteKind::GithubHttps, false) => "GitHub needs you to sign in. Add a GitHub token in App settings.",
        (Failure::SignIn, RemoteKind::OtherHttps, _) => {
            "The remote needs you to sign in, and packraven can't ask for a password. Set up a git credential helper (Git for Windows includes Git Credential Manager), or use an SSH remote."
        }
        (Failure::SignIn, RemoteKind::Ssh, _) => {
            "git couldn't sign in to the remote over SSH. Add your SSH key to your account on the host, or switch the remote to an HTTPS URL and add a GitHub token in App settings."
        }
        (Failure::Rejected, _, _) => "The remote has commits you don't have yet. Pull first, then push again.",
        (Failure::Unreachable, _, _) => "packraven couldn't reach the remote. Check your connection, then try again.",
    }
}

#[derive(Debug)]
pub struct ExplainedFailure(pub &'static str);

impl std::fmt::Display for ExplainedFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ExplainedFailure {}

fn failure(dir: &Path, args: &[&str], stderr: &str, auth: &RemoteAuth) -> anyhow::Error {
    match classify(stderr) {
        Some(failure) => {
            let kind = remote_url(dir).map_or(RemoteKind::OtherHttps, |url| remote_kind(&url));
            ExplainedFailure(explanation(&failure, &kind, auth.github_token.is_some())).into()
        }
        None => anyhow::anyhow!("git {} failed: {}", args.join(" "), stderr.trim()),
    }
}

pub fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let auth = RemoteAuth::load();
    let mut command = git::command();
    command.current_dir(dir).args(auth.config_args()).args(args).env("GIT_TERMINAL_PROMPT", "0");
    if let Some((name, value)) = auth.env() {
        command.env(name, value);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(failure(dir, args, &String::from_utf8_lossy(&output.stderr), &auth));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub async fn run_logged(dir: &Path, args: &[&str], log: &Log) -> Result<()> {
    log.info(format!("git {}", args.join(" ")));
    let auth = RemoteAuth::load();
    let mut command = Command::new(git::program());
    command.current_dir(dir).args(auth.config_args()).args(args).env("GIT_TERMINAL_PROMPT", "0").stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some((name, value)) = auth.env() {
        command.env(name, value);
    }
    crate::process::hide_window(&mut command);
    let mut child = command.spawn()?;
    let stdout = child.stdout.take();
    let stdout_log = log.clone();
    let stdout_task = tokio::spawn(async move {
        let Some(stdout) = stdout else { return };
        let mut reader = BufReader::new(stdout);
        while let Some(line) = crate::process::read_line_lossy(&mut reader).await {
            stdout_log.stream("out", &line);
        }
    });
    // We read stderr here rather than in a background task, so we have all of it before we explain a failure.
    let mut stderr = String::new();
    if let Some(pipe) = child.stderr.take() {
        let mut reader = BufReader::new(pipe);
        while let Some(line) = crate::process::read_line_lossy(&mut reader).await {
            log.stream("err", &line);
            stderr += &line;
            stderr.push('\n');
        }
    }
    let status = child.wait().await?;
    let _ = stdout_task.await;
    if !status.success() {
        return Err(failure(dir, args, &stderr, &auth));
    }
    Ok(())
}

pub fn is_explained(error: &anyhow::Error) -> bool {
    error.downcast_ref::<ExplainedFailure>().is_some()
}

#[derive(serde::Serialize)]
pub struct GithubIdentity {
    pub name: String,
    pub email: String,
}

#[derive(serde::Deserialize)]
struct GithubUser {
    id: u64,
    login: String,
    name: Option<String>,
    email: Option<String>,
}

// GitHub hides private emails from the API, so we fall back to the noreply address GitHub itself
// links to your account.
pub async fn github_identity() -> Result<GithubIdentity> {
    let Some(token) = credentials::get(Credential::Github) else { anyhow::bail!("Add a GitHub token first.") };
    let response = crate::http::CLIENT.get("https://api.github.com/user").bearer_auth(token).header("Accept", "application/vnd.github+json").send().await?;
    if matches!(response.status().as_u16(), 401 | 403) {
        anyhow::bail!("GitHub turned down the token. Check that it hasn't expired.");
    }
    let user: GithubUser = response.error_for_status()?.json().await?;
    Ok(GithubIdentity {
        name: user.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| user.login.clone()),
        email: user.email.filter(|e| !e.trim().is_empty()).unwrap_or_else(|| format!("{}+{}@users.noreply.github.com", user.id, user.login)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_git_failures() {
        assert_eq!(classify("fatal: Authentication failed for 'https://github.com/a/b.git/'"), Some(Failure::SignIn));
        assert_eq!(classify("fatal: could not read Username for 'https://github.com': terminal prompts disabled"), Some(Failure::SignIn));
        assert_eq!(classify("remote: Permission to a/b.git denied to someone.\nfatal: unable to access 'https://github.com/a/b.git/': The requested URL returned error: 403"), Some(Failure::SignIn));
        assert_eq!(classify("git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository."), Some(Failure::SignIn));
        assert_eq!(classify(" ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs"), Some(Failure::Rejected));
        assert_eq!(classify("fatal: unable to access 'https://github.com/a/b.git/': Could not resolve host: github.com"), Some(Failure::Unreachable));
        assert_eq!(classify("fatal: Not possible to fast-forward, aborting."), None);
    }

    #[test]
    fn tells_remote_kinds_apart() {
        assert_eq!(remote_kind("https://github.com/muon-rw/pack.git"), RemoteKind::GithubHttps);
        assert_eq!(remote_kind("https://gitlab.com/muon-rw/pack.git"), RemoteKind::OtherHttps);
        assert_eq!(remote_kind("git@github.com:muon-rw/pack.git"), RemoteKind::Ssh);
        assert_eq!(remote_kind("ssh://git@github.com/muon-rw/pack.git"), RemoteKind::Ssh);
    }

    #[test]
    fn token_reaches_git_only_for_github() {
        let auth = RemoteAuth { github_token: Some("secret-token".into()) };
        let fill = |host: &str| {
            let mut command = git::command();
            command.args(auth.config_args()).args(["credential", "fill"]).env("GIT_TERMINAL_PROMPT", "0").env(TOKEN_ENV, "secret-token");
            command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
            let mut child = command.spawn().unwrap();
            use std::io::Write;
            child.stdin.take().unwrap().write_all(format!("protocol=https\nhost={host}\n\n").as_bytes()).unwrap();
            String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
        };
        assert!(fill("github.com").contains("password=secret-token"));
        assert!(!fill("gitlab.com").contains("secret-token"));
        assert!(!auth.config_args().iter().any(|arg| arg.contains("secret-token")));
    }
}
