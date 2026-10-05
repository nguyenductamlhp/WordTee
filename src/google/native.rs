//! Google sign-in on desktop and Android: OAuth 2.0 for installed apps
//! (RFC 8252). The system browser does the signing in, and Google sends it
//! back to a one-off server on 127.0.0.1 with an authorization code. The code
//! is worthless without a secret that never leaves this process (PKCE,
//! RFC 7636), so another app catching it gains nothing.
//!
//! Android goes the same way, with the same "Desktop app" client. Google's own
//! Android sign-in needs Java (Credential Manager) and a client tied to the
//! APK's signing key, and this app has neither: there is no Java, and each
//! Docker build signs with a fresh key.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use eframe::egui;
use sha2::{Digest, Sha256};

use super::{Endpoints, Error, Event, Http, Profile, Request, Response, SCOPES, SyncWith, Token};
use crate::progress::Progress;

/// How long to wait for the browser to come back.
const SIGN_IN_WAIT: Duration = Duration::from_secs(10 * 60);

/// The "Desktop app" OAuth client. Google says of these that the secret is not
/// treated as one: it ships inside every copy of the app.
pub(super) struct Client {
    pub id: &'static str,
    pub secret: &'static str,
}

fn client() -> Option<Client> {
    let set = |value: Option<&'static str>| value.filter(|v| !v.is_empty());
    Some(Client {
        id: set(option_env!("WORDTEE_GOOGLE_DESKTOP_CLIENT_ID"))?,
        secret: set(option_env!("WORDTEE_GOOGLE_DESKTOP_CLIENT_SECRET"))?,
    })
}

pub(super) fn available() -> bool {
    client().is_some()
}

/// The refresh token gets a new access token whenever one is needed.
pub(super) fn can_sync_unattended(token: Option<&Token>, refresh_token: Option<&str>) -> bool {
    refresh_token.is_some() || token.is_some_and(Token::fresh)
}

/// Stops a sign-in that is waiting for the browser.
pub(super) struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Opens the browser at Google's sign-in, and waits on a thread for it to come
/// back.
pub(super) fn sign_in(
    ctx: &egui::Context,
    job: u64,
    events: Sender<Event>,
) -> Result<Cancel, Error> {
    let client = client().ok_or_else(|| Error::Failed("Google sign-in is not set up.".into()))?;
    let endpoints = Endpoints::google();
    let flow = Flow::start(&client, &endpoints).map_err(|e| Error::Failed(e.to_string()))?;
    ctx.open_url(egui::OpenUrl::new_tab(&flow.url));

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel = Cancel(cancelled.clone());
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let event = match block_on(flow.finish(&NativeHttp::new(), &client, &endpoints, &cancelled))
        {
            Ok((profile, token, refresh_token)) => Event::SignedIn {
                job,
                profile,
                token,
                refresh_token,
            },
            Err(error) => Event::Failed { job, error },
        };
        let _ = events.send(event);
        ctx.request_repaint();
    });
    Ok(cancel)
}

/// Syncs on a thread.
pub(super) fn sync(
    ctx: &egui::Context,
    job: u64,
    events: Sender<Event>,
    with: SyncWith,
    local: Progress,
) {
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let run = async {
            let client =
                client().ok_or_else(|| Error::Failed("Google sign-in is not set up.".into()))?;
            let endpoints = Endpoints::google();
            let http = NativeHttp::new();
            let token = match with.token.filter(Token::fresh) {
                Some(token) => token,
                None => {
                    let refresh_token = with.refresh_token.ok_or(Error::Expired)?;
                    refresh(&http, &client, &endpoints, &refresh_token).await?
                }
            };
            let merged = super::sync(&http, &endpoints, &token.access, local).await?;
            Ok((merged, token))
        };
        let event = match block_on(run) {
            Ok((merged, token)) => Event::Synced {
                job,
                merged: Box::new(merged),
                token,
            },
            Err(error) => Event::Failed { job, error },
        };
        let _ = events.send(event);
        ctx.request_repaint();
    });
}

/// Asks Google to drop the sign-in, without waiting to hear back.
pub(super) fn revoke(token: String) {
    std::thread::spawn(move || {
        let request = Request::form(Endpoints::google().revoke, &[("token", &token)]);
        if let Err(why) = block_on(NativeHttp::new().send(request)) {
            log::warn!("could not revoke the Google sign-in: {why}");
        }
    });
}

// -------------------------------------------------------------------------
// the sign-in
// -------------------------------------------------------------------------

/// One sign-in, from opening the browser to holding a token.
pub(super) struct Flow {
    listener: TcpListener,
    redirect_uri: String,
    verifier: String,
    state: String,
    /// Google's sign-in page, for the browser.
    pub url: String,
}

impl Flow {
    pub fn start(client: &Client, endpoints: &Endpoints) -> std::io::Result<Self> {
        // Loopback only, on a port the system picks: Google accepts any port
        // for a Desktop app client.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let redirect_uri = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
        let verifier = random_string(32)?;
        let state = random_string(16)?;
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("client_id", client.id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", SCOPES)
            .append_pair("code_challenge", &challenge(&verifier))
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            // A refresh token, so sync can run without asking again. Google
            // only gives one when it shows the consent screen, so ask for that
            // too: otherwise a second device would get none.
            .append_pair("access_type", "offline")
            .append_pair("prompt", "select_account consent")
            .finish();
        Ok(Self {
            url: format!("{}?{query}", endpoints.auth),
            listener,
            redirect_uri,
            verifier,
            state,
        })
    }

    /// Waits for the browser, then trades the code for tokens and finds out
    /// who signed in.
    pub async fn finish(
        self,
        http: &impl Http,
        client: &Client,
        endpoints: &Endpoints,
        cancelled: &AtomicBool,
    ) -> Result<(Profile, Token, Option<String>), Error> {
        let code = self.wait_for_code(cancelled)?;
        let request = Request::form(
            endpoints.token.clone(),
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("code_verifier", &self.verifier),
                ("redirect_uri", &self.redirect_uri),
                ("client_id", client.id),
                ("client_secret", client.secret),
            ],
        );
        let response = http.send(request).await?;
        if response.status != 200 {
            return Err(token_error(&response));
        }
        let (token, refresh_token) = super::parse_token(&response.body)?;
        let profile = super::user_info(http, endpoints, &token.access).await?;
        Ok((profile, token, refresh_token))
    }

    /// Serves the browser's visits until one brings a code for this sign-in.
    fn wait_for_code(&self, cancelled: &AtomicBool) -> Result<String, Error> {
        let io = |e: std::io::Error| Error::Failed(format!("Sign-in failed: {e}"));
        self.listener.set_nonblocking(true).map_err(io)?;
        let deadline = Instant::now() + SIGN_IN_WAIT;
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            if Instant::now() > deadline {
                return Err(Error::Failed(
                    "Timed out waiting for Google. Try signing in again.".into(),
                ));
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(result) = self.answer(stream) {
                        return result;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(io(e)),
            }
        }
    }

    /// Answers one visit. `None` for anything that is not Google coming back
    /// from this sign-in — a favicon request, or a stranger's — so the wait
    /// goes on.
    fn answer(&self, mut stream: TcpStream) -> Option<Result<String, Error>> {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        // Read the rest of the request head too, up to its blank line.
        // Closing a socket with bytes still unread in it sends a reset, and
        // the browser can show "connection reset" instead of the page.
        let mut header = String::new();
        while reader.read_line(&mut header).is_ok_and(|read| read > 2) {
            header.clear();
        }
        drop(reader);
        let Some(redirect) = Redirect::parse(&line) else {
            respond(&mut stream, "404 Not Found", "Not found", "");
            return None;
        };
        if redirect.state.as_deref() != Some(self.state.as_str()) {
            respond(&mut stream, "400 Bad Request", "Not this sign-in", "");
            return None;
        }
        Some(match (redirect.code, redirect.error) {
            (Some(code), _) => {
                respond(
                    &mut stream,
                    "200 OK",
                    "Signed in",
                    "You can close this tab and go back to WordTee.",
                );
                Ok(code)
            }
            (None, Some(error)) if error == "access_denied" => {
                respond(
                    &mut stream,
                    "200 OK",
                    "Sign-in cancelled",
                    "Nothing was changed.",
                );
                Err(Error::Cancelled)
            }
            (None, error) => {
                let error = error.unwrap_or_else(|| "no code".into());
                respond(&mut stream, "200 OK", "Sign-in failed", &error);
                Err(Error::Failed(format!(
                    "Google refused the sign-in: {error}"
                )))
            }
        })
    }
}

/// What Google's redirect back carries.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Redirect {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

impl Redirect {
    /// From an HTTP request line, `GET /?code=…&state=… HTTP/1.1`. `None`
    /// unless it is a GET of `/`.
    pub fn parse(request_line: &str) -> Option<Self> {
        let mut parts = request_line.split_whitespace();
        if parts.next()? != "GET" {
            return None;
        }
        let target = parts.next()?;
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        if path != "/" {
            return None;
        }
        let mut redirect = Self::default();
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            let slot = match &*key {
                "code" => &mut redirect.code,
                "state" => &mut redirect.state,
                "error" => &mut redirect.error,
                _ => continue,
            };
            *slot = Some(value.into_owned());
        }
        Some(redirect)
    }
}

/// The page the browser lands on.
fn respond(stream: &mut TcpStream, status: &str, title: &str, text: &str) {
    let back = back_to_app_link();
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>WordTee</title>\
         <body style=\"font-family:system-ui,sans-serif;max-width:28rem;margin:4rem auto;padding:0 1rem;text-align:center\">\
         <h1 style=\"font-size:1.4rem\">{}</h1><p>{}</p>{back}",
        escape(title),
        escape(text)
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

/// On Android, a button that brings the app back in front of the browser.
/// Chrome opens `intent:` links for a tap; the app's package is the process
/// name.
fn back_to_app_link() -> String {
    if !cfg!(target_os = "android") {
        return String::new();
    }
    let Ok(cmdline) = std::fs::read("/proc/self/cmdline") else {
        return String::new();
    };
    let name =
        String::from_utf8_lossy(cmdline.split(|&b| b == 0).next().unwrap_or_default()).into_owned();
    let package = name.split(':').next().unwrap_or_default();
    if package.is_empty()
        || !package
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
    {
        return String::new();
    }
    format!(
        "<p><a href=\"intent:#Intent;action=android.intent.action.MAIN;\
         category=android.intent.category.LAUNCHER;package={package};end\" \
         style=\"display:inline-block;padding:.7rem 1.4rem;border-radius:.5rem;\
         background:#0b6fc2;color:#fff;text-decoration:none\">Back to WordTee</a></p>"
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Gets a new access token with the refresh token.
pub(super) async fn refresh(
    http: &impl Http,
    client: &Client,
    endpoints: &Endpoints,
    refresh_token: &str,
) -> Result<Token, Error> {
    let request = Request::form(
        endpoints.token.clone(),
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client.id),
            ("client_secret", client.secret),
        ],
    );
    let response = http.send(request).await?;
    if response.status != 200 {
        return Err(token_error(&response));
    }
    Ok(super::parse_token(&response.body)?.0)
}

/// Reads a refusal from the token endpoint. `invalid_grant` means the refresh
/// token is dead — revoked by the user, or expired — so sign-in is over.
fn token_error(response: &Response) -> Error {
    #[derive(serde::Deserialize)]
    struct Refusal {
        error: String,
        error_description: Option<String>,
    }
    match serde_json::from_slice::<Refusal>(&response.body) {
        Ok(refusal) if refusal.error == "invalid_grant" => Error::SignedOut,
        Ok(refusal) => Error::Failed(format!(
            "Google refused the sign-in: {}",
            refusal.error_description.unwrap_or(refusal.error)
        )),
        Err(_) => Error::Failed(format!("Google answered {}", response.status)),
    }
}

/// `count` random bytes, URL-safe base64: the PKCE verifier and the `state`.
fn random_string(count: usize) -> std::io::Result<String> {
    let mut bytes = vec![0u8; count];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// PKCE's S256: what Google gets up front, in place of the verifier itself.
pub(super) fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

// -------------------------------------------------------------------------
// HTTP
// -------------------------------------------------------------------------

pub(super) struct NativeHttp(ureq::Agent);

impl NativeHttp {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(60)))
            // Google's error statuses carry a body worth reading.
            .http_status_as_error(false)
            .build();
        Self(agent.into())
    }
}

impl Http for NativeHttp {
    async fn send(&self, request: Request) -> Result<Response, String> {
        let mut builder = ureq::http::Request::builder()
            .method(request.method)
            .uri(&request.url);
        if let Some(token) = &request.bearer {
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }
        let result = match request.body {
            Some((content_type, bytes)) => self.0.run(
                builder
                    .header("Content-Type", content_type)
                    .body(bytes)
                    .map_err(|e| e.to_string())?,
            ),
            None => self.0.run(builder.body(()).map_err(|e| e.to_string())?),
        };
        let mut response = result.map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(64 << 20)
            .read_to_vec()
            .map_err(|e| e.to_string())?;
        Ok(Response { status, body })
    }
}

/// Runs a future to completion on this thread. [`NativeHttp`] blocks rather
/// than waits, so the future is never pending and this polls it once.
pub(super) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::yield_now();
    }
}
