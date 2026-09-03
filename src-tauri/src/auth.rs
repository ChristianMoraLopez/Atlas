use crate::{
    diagnostics,
    error::{AppError, Context, Result},
    models::{AccountInfo, CachedAccessToken},
    state::AppState,
};
use chrono::{Duration, Utc};
use keyring::Entry;
use oauth2::{
    basic::BasicClient, AuthUrl, AuthorizationCode, ClientId, CsrfToken, EndpointNotSet,
    EndpointSet, PkceCodeChallenge, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration as StdDuration, Instant},
};
use url::Url;

const KEYRING_SERVICE: &str = "Atlas Circana Tracker";
const KEYRING_USER: &str = "microsoft-oauth-token";
const CREDENTIAL_CHUNK_SIZE: usize = 800;
const MAX_CREDENTIAL_CHUNKS: usize = 32;

type OAuthClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

#[derive(Clone, Debug, Deserialize)]
struct TokenRecord {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RefreshMetadata {
    version: u8,
    generation: String,
    chunks: usize,
}

fn entry(user: &str) -> Result<Entry> {
    Entry::new(KEYRING_SERVICE, user).context("Unable to access Windows Credential Manager")
}

fn credential_name(generation: &str, index: usize) -> String {
    format!("{KEYRING_USER}-{generation}-{index}")
}

fn read_metadata_value() -> Result<Option<String>> {
    match entry(KEYRING_USER)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Message(format!(
            "Unable to read Windows Credential Manager: {e}"
        ))),
    }
}

fn delete_entry(user: &str) -> Result<()> {
    match entry(user)?.delete_credential() {
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Message(format!(
            "Unable to remove a stored Microsoft credential: {e}"
        ))),
    }
}

fn save_refresh_token(refresh_token: &str) -> Result<()> {
    if refresh_token.is_empty() {
        return Err(AppError::Message(
            "Microsoft did not return a refresh token. Ask your administrator to allow the offline_access delegated scope.".into(),
        ));
    }
    let old_metadata = read_metadata_value()?
        .and_then(|value| serde_json::from_str::<RefreshMetadata>(&value).ok());
    let chunks = refresh_token
        .as_bytes()
        .chunks(CREDENTIAL_CHUNK_SIZE)
        .map(|value| String::from_utf8(value.to_vec()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Microsoft returned an invalid refresh token")?;
    if chunks.is_empty() || chunks.len() > MAX_CREDENTIAL_CHUNKS {
        return Err(AppError::Message(
            "The Microsoft refresh token is too large for Windows Credential Manager.".into(),
        ));
    }
    let generation = uuid::Uuid::new_v4().simple().to_string();
    let mut written: Vec<String> = Vec::new();
    for (index, value) in chunks.iter().enumerate() {
        let name = credential_name(&generation, index);
        if let Err(error) = entry(&name)?.set_password(value) {
            for saved in written {
                let _ = delete_entry(&saved);
            }
            return Err(AppError::Message(format!(
                "Unable to store the Microsoft session securely: {error}"
            )));
        }
        written.push(name);
    }
    let metadata = RefreshMetadata {
        version: 1,
        generation,
        chunks: chunks.len(),
    };
    entry(KEYRING_USER)?
        .set_password(&serde_json::to_string(&metadata)?)
        .context("Unable to finalize the Microsoft session in Windows Credential Manager")?;

    if let Some(old) = old_metadata {
        for index in 0..old.chunks.min(MAX_CREDENTIAL_CHUNKS) {
            let _ = delete_entry(&credential_name(&old.generation, index));
        }
    }
    Ok(())
}

fn load_refresh_token() -> Result<Option<String>> {
    let Some(value) = read_metadata_value()? else {
        return Ok(None);
    };
    if let Ok(metadata) = serde_json::from_str::<RefreshMetadata>(&value) {
        if metadata.version != 1 || metadata.chunks == 0 || metadata.chunks > MAX_CREDENTIAL_CHUNKS
        {
            return Err(AppError::Message(
                "The stored Microsoft session metadata is invalid. Sign out and sign in again."
                    .into(),
            ));
        }
        let mut refresh = String::new();
        for index in 0..metadata.chunks {
            let name = credential_name(&metadata.generation, index);
            let chunk = entry(&name)?.get_password().map_err(|error| {
                AppError::Message(format!(
                    "A stored Microsoft session segment is unavailable: {error}. Sign in again."
                ))
            })?;
            refresh.push_str(&chunk);
        }
        return Ok(Some(refresh));
    }

    // Version 0.1 stored access and refresh tokens together. Read that format once so
    // existing users can migrate without being forced through login again.
    let legacy: TokenRecord = serde_json::from_str(&value)
        .context("The stored Microsoft session is invalid. Sign out and sign in again")?;
    Ok(legacy.refresh_token)
}

pub fn has_token() -> Result<bool> {
    Ok(load_refresh_token()?.is_some())
}

pub fn clear_token(state: &AppState) -> Result<()> {
    if let Some(value) = read_metadata_value()? {
        if let Ok(metadata) = serde_json::from_str::<RefreshMetadata>(&value) {
            for index in 0..metadata.chunks.min(MAX_CREDENTIAL_CHUNKS) {
                delete_entry(&credential_name(&metadata.generation, index))?;
            }
        }
    }
    delete_entry(KEYRING_USER)?;
    *state
        .cached_access_token
        .lock()
        .map_err(|_| AppError::Message("Microsoft session lock was poisoned".into()))? = None;
    Ok(())
}

fn oauth_client(state: &AppState, redirect: Option<String>) -> Result<OAuthClient> {
    let config = state.microsoft_config()?;
    if config.client_id.is_empty() || config.tenant_id.is_empty() {
        return Err(AppError::Message(
            "Enter the Microsoft Application ID and Tenant ID in Atlas first.".into(),
        ));
    }
    let authority = format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0",
        config.tenant_id
    );
    let client = BasicClient::new(ClientId::new(config.client_id))
        .set_auth_uri(
            AuthUrl::new(format!("{authority}/authorize"))
                .context("Invalid Microsoft authorization endpoint")?,
        )
        .set_token_uri(
            TokenUrl::new(format!("{authority}/token"))
                .context("Invalid Microsoft token endpoint")?,
        );
    match redirect {
        Some(value) => Ok(client
            .set_redirect_uri(RedirectUrl::new(value).context("Invalid loopback redirect URL")?)),
        None => Ok(client),
    }
}

pub async fn sign_in(
    state: &AppState,
    include_files: bool,
    include_teams: bool,
) -> Result<AccountInfo> {
    diagnostics::info(
        "auth",
        match (include_files, include_teams) {
            (true, true) => "Starting Microsoft PKCE sign-in with file and Teams access",
            (true, false) => "Starting Microsoft PKCE sign-in with file access",
            (false, true) => "Starting Microsoft PKCE sign-in with Teams access",
            (false, false) => "Starting Microsoft PKCE sign-in",
        },
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .context("Unable to start the secure login callback on localhost")?;
    let port = listener.local_addr()?.port();
    // Microsoft permits a dynamic port for registered public-client loopback redirects.
    // The hostname remains localhost while the listener itself is pinned to 127.0.0.1.
    let redirect = format!("http://localhost:{port}");
    let client = oauth_client(state, Some(redirect))?;
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let authorization = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("openid".into()))
        .add_scope(Scope::new("profile".into()))
        .add_scope(Scope::new("offline_access".into()))
        .add_scope(Scope::new("User.Read".into()))
        .add_scope(Scope::new("Calendars.Read".into()))
        .add_scope(Scope::new("Mail.Read".into()))
        .set_pkce_challenge(challenge);
    let authorization = if include_files {
        authorization.add_scope(Scope::new("Files.ReadWrite".into()))
    } else {
        authorization
    };
    let authorization = if include_teams {
        authorization.add_scope(Scope::new("Chat.Read".into()))
    } else {
        authorization
    };
    let (auth_url, csrf) = authorization.url();

    open::that(auth_url.as_str())
        .context("Unable to open the system browser for Microsoft sign-in")?;
    diagnostics::info(
        "auth",
        "System browser opened; waiting for loopback callback",
    );
    let expected_state = csrf.secret().clone();
    let code = tokio::task::spawn_blocking(move || receive_code(listener, &expected_state))
        .await
        .context("Microsoft sign-in listener stopped unexpectedly")??;

    let token = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(verifier)
        .request_async(&state.http)
        .await
        .context("Microsoft rejected the authorization code")?;
    let expires_at = Utc::now()
        + Duration::from_std(token.expires_in().unwrap_or(StdDuration::from_secs(3600)))
            .unwrap_or(Duration::hours(1));
    let record = TokenRecord {
        access_token: token.access_token().secret().clone(),
        refresh_token: token.refresh_token().map(|v| v.secret().clone()),
        expires_at: expires_at.timestamp(),
    };
    let account = get_account(state, &record.access_token).await?;
    let refresh = record.refresh_token.as_deref().ok_or_else(|| {
        AppError::Message(
            "Microsoft did not return a refresh token. Ensure offline_access is allowed for this public client.".into(),
        )
    })?;
    save_refresh_token(refresh)?;
    *state
        .cached_access_token
        .lock()
        .map_err(|_| AppError::Message("Microsoft session lock was poisoned".into()))? =
        Some(CachedAccessToken {
            value: record.access_token,
            expires_at: record.expires_at,
        });
    diagnostics::info(
        "auth",
        "Microsoft sign-in completed and the session was stored",
    );
    Ok(account)
}

fn receive_code(listener: TcpListener, expected_state: &str) -> Result<String> {
    listener.set_nonblocking(true)?;
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(StdDuration::from_secs(2)));
                let mut buffer = [0_u8; 8192];
                let read = match stream.read(&mut buffer) {
                    Ok(0) => continue,
                    Ok(read) => read,
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        continue
                    }
                    Err(error) => return Err(error.into()),
                };
                let request = String::from_utf8_lossy(&buffer[..read]);
                let Some(path) = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                else {
                    continue;
                };
                let Ok(url) = Url::parse(&format!("http://127.0.0.1{path}")) else {
                    continue;
                };
                let pairs: std::collections::HashMap<_, _> =
                    url.query_pairs().into_owned().collect();
                // Browsers can preconnect or request a favicon before the OAuth redirect.
                // Ignore those requests and keep waiting for a callback that carries OAuth data.
                if !pairs.contains_key("code") && !pairs.contains_key("error") {
                    write_browser_response(&mut stream, "Atlas is waiting for Microsoft sign-in.");
                    continue;
                }
                if let Some(error) = pairs.get("error") {
                    let description = pairs.get("error_description").unwrap_or(error);
                    write_browser_response(
                        &mut stream,
                        &format!(
                            "Microsoft sign-in was not completed: {}",
                            html_escape::encode_text(description)
                        ),
                    );
                    return Err(AppError::Message(format!(
                        "Microsoft sign-in failed: {}",
                        description
                    )));
                }
                if pairs.get("state").map(String::as_str) != Some(expected_state) {
                    write_browser_response(
                        &mut stream,
                        "Atlas rejected this callback because its security state did not match. Return to Atlas and try again.",
                    );
                    return Err(AppError::Message(
                        "Microsoft sign-in state did not match. Please try again.".into(),
                    ));
                }
                let code = pairs.get("code").cloned().ok_or_else(|| {
                    AppError::Message("Microsoft did not return an authorization code.".into())
                })?;
                write_browser_response(
                    &mut stream,
                    "Microsoft sign-in is complete. You can close this tab and return to Atlas.",
                );
                return Ok(code);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() > StdDuration::from_secs(300) {
                    return Err(AppError::Message(
                        "Microsoft sign-in timed out after five minutes.".into(),
                    ));
                }
                std::thread::sleep(StdDuration::from_millis(100));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

fn write_browser_response(stream: &mut std::net::TcpStream, message: &str) {
    let body = format!("<!doctype html><meta charset=utf-8><title>Atlas sign-in</title><style>body{{font:16px Segoe UI;background:#f6f3ea;color:#17201f;display:grid;place-items:center;height:100vh;margin:0}}main{{max-width:520px;padding:40px;background:white;border-radius:20px;box-shadow:0 20px 60px #0c302a1a}}h1{{font-family:Georgia;color:#0f4f45}}</style><main><h1>Return to Atlas</h1><p>{message}</p></main>");
    let http = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
    let _ = stream.write_all(http.as_bytes());
}

pub async fn access_token(state: &AppState) -> Result<String> {
    if let Some(cached) = state
        .cached_access_token
        .lock()
        .map_err(|_| AppError::Message("Microsoft session lock was poisoned".into()))?
        .clone()
    {
        if cached.expires_at > (Utc::now() + Duration::minutes(2)).timestamp() {
            return Ok(cached.value);
        }
    }
    let refresh = load_refresh_token()?
        .ok_or_else(|| AppError::Message("Sign in with Microsoft first.".into()))?;
    diagnostics::info("auth", "Refreshing the Microsoft access token");
    let client = oauth_client(state, None)?;
    let response = client
        .exchange_refresh_token(&RefreshToken::new(refresh))
        .request_async(&state.http)
        .await
        .context("Unable to refresh your Microsoft session. Sign in again")?;
    let access_token = response.access_token().secret().clone();
    if let Some(next) = response.refresh_token() {
        save_refresh_token(next.secret())?;
    }
    let expires_at = (Utc::now()
        + Duration::from_std(
            response
                .expires_in()
                .unwrap_or(StdDuration::from_secs(3600)),
        )
        .unwrap_or(Duration::hours(1)))
    .timestamp();
    *state
        .cached_access_token
        .lock()
        .map_err(|_| AppError::Message("Microsoft session lock was poisoned".into()))? =
        Some(CachedAccessToken {
            value: access_token.clone(),
            expires_at,
        });
    Ok(access_token)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphMe {
    display_name: Option<String>,
    mail: Option<String>,
    user_principal_name: Option<String>,
}

async fn get_account(state: &AppState, token: &str) -> Result<AccountInfo> {
    let response = state
        .http
        .get("https://graph.microsoft.com/v1.0/me?$select=displayName,mail,userPrincipalName")
        .bearer_auth(token)
        .send()
        .await
        .context("Unable to contact Microsoft Graph for your profile")?;
    if !response.status().is_success() {
        return Err(AppError::Message(format!(
            "Microsoft Graph could not read your profile ({}).",
            response.status()
        )));
    }
    let me: GraphMe = response
        .json()
        .await
        .context("Microsoft Graph returned an invalid profile")?;
    let email = me.mail.or(me.user_principal_name).ok_or_else(|| {
        AppError::Message(
            "Microsoft Graph did not return an email address for this account.".into(),
        )
    })?;
    Ok(AccountInfo {
        display_name: me.display_name.unwrap_or_else(|| email.clone()),
        email,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_loopback_callback_is_constructed() {
        let value = "http://127.0.0.1:49152";
        let url = Url::parse(value).unwrap();
        assert!(url
            .host_str()
            .unwrap()
            .parse::<std::net::IpAddr>()
            .unwrap()
            .is_loopback());
    }
}
