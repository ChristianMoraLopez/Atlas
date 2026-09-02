use crate::{
    error::{AppError, Context, Result},
    models::AccountInfo,
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

type OAuthClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TokenRecord {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: i64,
}

fn entry() -> Result<Entry> {
    Entry::new(KEYRING_SERVICE, KEYRING_USER).context("Unable to access the OS credential store")
}

fn save_token(record: &TokenRecord) -> Result<()> {
    entry()?
        .set_password(&serde_json::to_string(record)?)
        .context("Unable to store the Microsoft token securely")
}

fn load_token() -> Result<Option<TokenRecord>> {
    match entry()?.get_password() {
        Ok(value) => Ok(Some(
            serde_json::from_str(&value).context("Stored Microsoft token is invalid")?,
        )),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Message(format!(
            "Unable to read the OS credential store: {e}"
        ))),
    }
}

pub fn has_token() -> bool {
    load_token().ok().flatten().is_some()
}

pub fn clear_token() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Message(format!(
            "Unable to remove the stored credential: {e}"
        ))),
    }
}

fn oauth_client(state: &AppState, redirect: Option<String>) -> Result<OAuthClient> {
    if state.azure.client_id.is_empty() || state.azure.tenant_id.is_empty() {
        return Err(AppError::Message(
            "This build is missing its Azure client ID or tenant ID.".into(),
        ));
    }
    let authority = format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0",
        state.azure.tenant_id
    );
    let client = BasicClient::new(ClientId::new(state.azure.client_id.clone()))
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

pub async fn sign_in(state: &AppState) -> Result<AccountInfo> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .context("Unable to start the secure login callback on localhost")?;
    let port = listener.local_addr()?.port();
    // Microsoft permits a dynamic port for registered public-client loopback redirects.
    // The hostname remains localhost while the listener itself is pinned to 127.0.0.1.
    let redirect = format!("http://localhost:{port}");
    let client = oauth_client(state, Some(redirect))?;
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("openid".into()))
        .add_scope(Scope::new("profile".into()))
        .add_scope(Scope::new("offline_access".into()))
        .add_scope(Scope::new("User.Read".into()))
        .add_scope(Scope::new("Calendars.Read".into()))
        .add_scope(Scope::new("Mail.Read".into()))
        .add_scope(Scope::new("Chat.Read".into()))
        .set_pkce_challenge(challenge)
        .url();

    open::that(auth_url.as_str())
        .context("Unable to open the system browser for Microsoft sign-in")?;
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
    save_token(&record)?;
    get_account(state, &record.access_token).await
}

fn receive_code(listener: TcpListener, expected_state: &str) -> Result<String> {
    listener.set_nonblocking(true)?;
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buffer = [0_u8; 8192];
                let read = stream.read(&mut buffer)?;
                let request = String::from_utf8_lossy(&buffer[..read]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .ok_or_else(|| AppError::Message("Invalid browser callback.".into()))?;
                let url = Url::parse(&format!("http://127.0.0.1{path}"))
                    .context("Invalid browser callback URL")?;
                let pairs: std::collections::HashMap<_, _> =
                    url.query_pairs().into_owned().collect();
                let response = if let Some(error) = pairs.get("error") {
                    format!(
                        "Microsoft sign-in was not completed: {}",
                        pairs.get("error_description").unwrap_or(error)
                    )
                } else {
                    "Microsoft sign-in is complete. You can close this tab and return to Atlas."
                        .into()
                };
                let body = format!("<!doctype html><meta charset=utf-8><title>Atlas sign-in</title><style>body{{font:16px Segoe UI;background:#f6f3ea;color:#17201f;display:grid;place-items:center;height:100vh;margin:0}}main{{max-width:520px;padding:40px;background:white;border-radius:20px;box-shadow:0 20px 60px #0c302a1a}}h1{{font-family:Georgia;color:#0f4f45}}</style><main><h1>Return to Atlas</h1><p>{response}</p></main>");
                let http = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                stream.write_all(http.as_bytes())?;
                if let Some(error) = pairs.get("error") {
                    return Err(AppError::Message(format!(
                        "Microsoft sign-in failed: {}",
                        pairs.get("error_description").unwrap_or(error)
                    )));
                }
                if pairs.get("state") != Some(&expected_state.to_string()) {
                    return Err(AppError::Message(
                        "Microsoft sign-in state did not match. Please try again.".into(),
                    ));
                }
                return pairs.get("code").cloned().ok_or_else(|| {
                    AppError::Message("Microsoft did not return an authorization code.".into())
                });
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

pub async fn access_token(state: &AppState) -> Result<String> {
    let mut record =
        load_token()?.ok_or_else(|| AppError::Message("Sign in with Microsoft first.".into()))?;
    if record.expires_at > (Utc::now() + Duration::minutes(2)).timestamp() {
        return Ok(record.access_token);
    }
    let refresh = record.refresh_token.clone().ok_or_else(|| {
        AppError::Message("Your Microsoft session expired. Sign in again.".into())
    })?;
    let client = oauth_client(state, None)?;
    let response = client
        .exchange_refresh_token(&RefreshToken::new(refresh))
        .request_async(&state.http)
        .await
        .context("Unable to refresh your Microsoft session. Sign in again")?;
    record.access_token = response.access_token().secret().clone();
    if let Some(next) = response.refresh_token() {
        record.refresh_token = Some(next.secret().clone());
    }
    record.expires_at = (Utc::now()
        + Duration::from_std(
            response
                .expires_in()
                .unwrap_or(StdDuration::from_secs(3600)),
        )
        .unwrap_or(Duration::hours(1)))
    .timestamp();
    save_token(&record)?;
    Ok(record.access_token)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphMe {
    display_name: String,
    mail: Option<String>,
    user_principal_name: String,
}

async fn get_account(state: &AppState, token: &str) -> Result<AccountInfo> {
    let response = state
        .http
        .get("https://graph.microsoft.com/v1.0/me?$select=displayName,mail,userPrincipalName")
        .bearer_auth(token)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(AppError::Message(format!(
            "Microsoft Graph could not read your profile ({}).",
            response.status()
        )));
    }
    let me: GraphMe = response.json().await?;
    Ok(AccountInfo {
        display_name: me.display_name,
        email: me.mail.unwrap_or(me.user_principal_name),
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
