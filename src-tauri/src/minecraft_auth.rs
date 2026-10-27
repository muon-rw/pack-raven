use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Mutex};

use crate::credentials::config_dir;
use crate::http::CLIENT;
use crate::keychain;
use crate::launch::Player;

const CLIENT_ID: &str = "758fd383-0e8c-4dee-8d47-e529cb8ddf2b";
// XboxLive.signin only works through the consumers tenant, so work and school accounts can't sign in.
const AUTHORIZE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const SCOPE: &str = "XboxLive.signin offline_access";
const XBOX_USER_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const ENTITLEMENTS_URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const KEYCHAIN_USER: &str = "microsoft-refresh-token";
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
// We renew a little early, so a token can't expire between a test run's start and the client's launch.
const RENEW_BEFORE_EXPIRY: Duration = Duration::from_secs(600);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub name: String,
    pub uuid: String,
}

#[derive(Clone)]
pub struct Session {
    pub account: Account,
    access_token: String,
    expires_at: Instant,
}

impl Session {
    pub fn player(&self) -> Player {
        Player { name: self.account.name.clone(), uuid: self.account.uuid.clone(), access_token: self.access_token.clone(), user_type: "msa" }
    }
}

pub type SessionCache = Mutex<Option<Session>>;

#[derive(Deserialize)]
struct MicrosoftTokens {
    access_token: String,
    refresh_token: String,
}

fn account_path() -> PathBuf {
    config_dir().join("minecraft-account.json")
}

fn stored_refresh_token() -> Option<String> {
    keychain::get(KEYCHAIN_USER).filter(|t| !t.is_empty())
}

pub fn account() -> Option<Account> {
    stored_refresh_token()?;
    serde_json::from_str(&std::fs::read_to_string(account_path()).ok()?).ok()
}

pub async fn sign_in(open: impl FnOnce(&str) -> Result<()>, cancel: oneshot::Receiver<()>) -> Result<Option<Session>> {
    let listener = TcpListener::bind("127.0.0.1:0").await.context("opening a local port for the Microsoft sign-in")?;
    let redirect = format!("http://localhost:{}", listener.local_addr()?.port());
    let verifier = random_token()?;
    let state = random_token()?;
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let url = reqwest::Url::parse_with_params(
        AUTHORIZE_URL,
        [
            ("client_id", CLIENT_ID),
            ("response_type", "code"),
            ("redirect_uri", &redirect),
            ("scope", SCOPE),
            ("state", &state),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("prompt", "select_account"),
        ],
    )?;
    open(url.as_str())?;
    let code = tokio::select! {
        code = receive_code(&listener, &state) => code?,
        Ok(()) = cancel => return Ok(None),
        _ = tokio::time::sleep(SIGN_IN_TIMEOUT) => bail!("The Microsoft sign-in timed out. Try again."),
    };
    let tokens = microsoft_tokens(&[("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", &redirect), ("code_verifier", &verifier)]).await?;
    finish(tokens).await.map(Some)
}

pub async fn session(cache: &SessionCache) -> Result<Session> {
    let mut cached = cache.lock().await;
    if let Some(session) = cached.as_ref().filter(|s| s.expires_at > Instant::now() + RENEW_BEFORE_EXPIRY) {
        return Ok(session.clone());
    }
    let refresh_token = stored_refresh_token().context("You're not signed in to Minecraft. Sign in under App settings.")?;
    let tokens = microsoft_tokens(&[("grant_type", "refresh_token"), ("refresh_token", &refresh_token)])
        .await
        .context("Your Microsoft sign-in has expired. Sign in again under App settings.")?;
    let session = finish(tokens).await?;
    *cached = Some(session.clone());
    Ok(session)
}

pub async fn sign_out(cache: &SessionCache) -> Result<()> {
    cache.lock().await.take();
    keychain::remove(KEYCHAIN_USER).context("removing your Microsoft sign-in from the system keychain")?;
    match std::fs::remove_file(account_path()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).context("removing minecraft-account.json"),
        _ => Ok(()),
    }
}

fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("couldn't get random bytes from the system: {e}"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

const SIGNED_IN_PAGE: &str = "You're signed in. You can close this tab and go back to packraven.";
const FAILED_PAGE: &str = "Microsoft didn't sign you in. Go back to packraven to try again.";

async fn receive_code(listener: &TcpListener, state: &str) -> Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buffer = vec![0; 8192];
        let read = stream.read(&mut buffer).await.unwrap_or(0);
        let request = String::from_utf8_lossy(&buffer[..read]);
        let target = request.lines().next().and_then(|line| line.strip_prefix("GET ")).and_then(|rest| rest.split(' ').next()).unwrap_or("/");
        let query: HashMap<String, String> = reqwest::Url::parse("http://localhost")?.join(target)?.query_pairs().into_owned().collect();
        // Browsers also ask for /favicon.ico, and anything else on this PC can reach the port.
        if query.get("state").map(String::as_str) != Some(state) {
            respond(&mut stream, "404 Not Found", "").await;
            continue;
        }
        if let Some(code) = query.get("code") {
            respond(&mut stream, "200 OK", SIGNED_IN_PAGE).await;
            return Ok(code.clone());
        }
        respond(&mut stream, "200 OK", FAILED_PAGE).await;
        let reason = query.get("error_description").or(query.get("error")).map_or("Microsoft gave no reason", String::as_str);
        bail!("Microsoft didn't sign you in: {reason}");
    }
}

async fn respond(stream: &mut TcpStream, status: &str, message: &str) {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>packraven</title><body style=\"font:16px system-ui;background:#f7f5ef;color:#1e211b;padding:48px\"><p>{message}</p>"
    );
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

async fn microsoft_tokens(grant: &[(&str, &str)]) -> Result<MicrosoftTokens> {
    let mut form = vec![("client_id", CLIENT_ID), ("scope", SCOPE)];
    form.extend_from_slice(grant);
    let response = CLIENT.post(TOKEN_URL).form(&form).send().await?;
    if !response.status().is_success() {
        let body: Value = response.json().await.unwrap_or_default();
        bail!("Microsoft refused the sign-in: {}", body["error_description"].as_str().or(body["error"].as_str()).unwrap_or("no reason given"));
    }
    Ok(response.json().await?)
}

// Microsoft rotates the refresh token on every use, so we save the new one each time.
async fn finish(tokens: MicrosoftTokens) -> Result<Session> {
    let session = minecraft_session(&tokens.access_token).await?;
    keychain::set(KEYCHAIN_USER, &tokens.refresh_token).context("saving your Microsoft sign-in to the system keychain")?;
    let path = account_path();
    std::fs::create_dir_all(path.parent().context("the config folder has no parent")?)?;
    std::fs::write(path, serde_json::to_string_pretty(&session.account)?).context("writing minecraft-account.json")?;
    Ok(session)
}

async fn minecraft_session(microsoft_token: &str) -> Result<Session> {
    let xbox: Value = CLIENT
        .post(XBOX_USER_URL)
        .json(&json!({
            "Properties": { "AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("d={microsoft_token}") },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))
        .send()
        .await?
        .error_for_status()
        .context("Xbox Live didn't accept the Microsoft sign-in")?
        .json()
        .await?;
    let xbox_token = xbox["Token"].as_str().context("Xbox Live sent no token")?;

    let response = CLIENT
        .post(XSTS_URL)
        .json(&json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbox_token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }))
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        let body: Value = response.json().await.unwrap_or_default();
        bail!(xsts_problem(body["XErr"].as_u64().unwrap_or(0)));
    }
    let xsts: Value = response.error_for_status().context("Xbox Live didn't authorize this account")?.json().await?;
    let xsts_token = xsts["Token"].as_str().context("Xbox Live sent no XSTS token")?;
    let user_hash = xsts.pointer("/DisplayClaims/xui/0/uhs").and_then(Value::as_str).context("Xbox Live sent no user hash")?;

    let response = CLIENT.post(MINECRAFT_LOGIN_URL).json(&json!({ "identityToken": format!("XBL3.0 x={user_hash};{xsts_token}") })).send().await?;
    if response.status() == reqwest::StatusCode::FORBIDDEN {
        bail!("Minecraft refused packraven's sign-in. We're still waiting on approval from Mojang for our Microsoft app, so this isn't working yet.");
    }
    let login: Value = response.error_for_status().context("Minecraft didn't accept the Xbox Live sign-in")?.json().await?;
    let access_token = login["access_token"].as_str().context("Minecraft sent no access token")?.to_string();
    let expires_in = login["expires_in"].as_u64().unwrap_or(86_400);

    let entitlements: Value = CLIENT.get(ENTITLEMENTS_URL).bearer_auth(&access_token).send().await?.error_for_status()?.json().await?;
    let owns_java = entitlements["items"].as_array().into_iter().flatten().any(|item| matches!(item["name"].as_str(), Some("game_minecraft" | "product_minecraft")));
    if !owns_java {
        bail!("This Microsoft account doesn't own Minecraft: Java Edition.");
    }

    let response = CLIENT.get(PROFILE_URL).bearer_auth(&access_token).send().await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        bail!("This account has no Minecraft profile yet. Open the official Minecraft Launcher once to create one, then sign in again.");
    }
    let profile: Value = response.error_for_status().context("Minecraft didn't send your profile")?.json().await?;
    let account = Account {
        name: profile["name"].as_str().context("your Minecraft profile has no name")?.to_string(),
        uuid: profile["id"].as_str().context("your Minecraft profile has no UUID")?.to_string(),
    };
    Ok(Session { account, access_token, expires_at: Instant::now() + Duration::from_secs(expires_in) })
}

fn xsts_problem(code: u64) -> String {
    match code {
        2148916227 => "Xbox has banned this account, so Minecraft can't sign it in.".into(),
        2148916233 => "This Microsoft account has no Xbox profile. Sign in once at minecraft.net to create one, then try again.".into(),
        2148916235 => "Xbox Live isn't available in this account's country, so Minecraft can't sign it in.".into(),
        2148916236 | 2148916237 => "Xbox needs adult verification for this account. Verify it on the Xbox website, then try again.".into(),
        2148916238 => "This is a child account. An adult has to add it to a Microsoft family before it can sign in to Minecraft.".into(),
        code => format!("Xbox Live didn't authorize this account (error {code})."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_tokens_suit_pkce() {
        let token = random_token().unwrap();
        // RFC 7636 wants a verifier of 43 to 128 characters.
        assert_eq!(token.len(), 43);
        assert_ne!(token, random_token().unwrap());
    }

    #[tokio::test]
    async fn receives_the_code_only_with_our_state() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let browser = tokio::spawn(async move {
            for path in ["/favicon.ico", "/?code=wrong&state=other", "/?code=abc&state=ours"] {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes()).await.unwrap();
                let mut reply = String::new();
                stream.read_to_string(&mut reply).await.unwrap();
            }
        });
        assert_eq!(receive_code(&listener, "ours").await.unwrap(), "abc");
        browser.await.unwrap();
    }
}
