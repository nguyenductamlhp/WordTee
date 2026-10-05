//! Sign in with Google, and keep a copy of the progress in the user's Google
//! Drive so it follows them to their other devices.
//!
//! There is still no server. The copy is one file in the Drive's app data
//! folder — a hidden area only this app can see, which is all the
//! `drive.appdata` scope reaches — and each device folds it into its own
//! progress with [`Progress::merge`]. Signing in is optional; without it the
//! app makes no network calls at all.
//!
//! Signing in differs by platform, the rest is shared:
//!
//! | Platform | Signing in | Google OAuth client type |
//! | --- | --- | --- |
//! | Desktop, Android | the system browser, back to a one-off server on 127.0.0.1 (`native.rs`) | Desktop app |
//! | Web | Google's own popup, from its script in `web/index.html` (`web.rs`) | Web application |
//!
//! The client IDs are compiled in from environment variables (see the README).
//! A build without them says sync is not set up.

use std::sync::mpsc;
use std::time::Duration;

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::progress::{self, Progress, Stamp};

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
use native as platform;

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
use web as platform;

/// Who the user is, and the app's own folder in their Drive — nothing else in
/// it.
const SCOPES: &str = "openid email profile https://www.googleapis.com/auth/drive.appdata";
const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive.appdata";

/// The one file sync keeps, in the app data folder.
const FILE_NAME: &str = "progress.json";

/// Key the account is stored under, next to the progress.
const STORAGE_KEY: &str = "wordtee.google";

/// While there are changes Drive has not seen, sync at most this often.
const SYNC_EVERY: f64 = 120.0;

/// Where Google's services are. Not constants, so that the tests can stand in
/// for them. The web uses only the APIs; Google's script does the OAuth part.
#[derive(Clone)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) struct Endpoints {
    pub auth: String,
    pub token: String,
    pub revoke: String,
    pub userinfo: String,
    pub drive: String,
    pub upload: String,
}

impl Endpoints {
    pub fn google() -> Self {
        Self {
            auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            revoke: "https://oauth2.googleapis.com/revoke".into(),
            userinfo: "https://openidconnect.googleapis.com/v1/userinfo".into(),
            drive: "https://www.googleapis.com/drive/v3".into(),
            upload: "https://www.googleapis.com/upload/drive/v3".into(),
        }
    }
}

// -------------------------------------------------------------------------
// HTTP, as each platform provides it
// -------------------------------------------------------------------------

pub(crate) struct Request {
    pub method: &'static str,
    pub url: String,
    pub bearer: Option<String>,
    /// Content type and bytes.
    pub body: Option<(String, Vec<u8>)>,
}

impl Request {
    fn get(url: String, token: &str) -> Self {
        Self {
            method: "GET",
            url,
            bearer: Some(token.to_owned()),
            body: None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn form(url: String, fields: &[(&str, &str)]) -> Self {
        let body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields)
            .finish();
        Self {
            method: "POST",
            url,
            bearer: None,
            body: Some((
                "application/x-www-form-urlencoded".into(),
                body.into_bytes(),
            )),
        }
    }
}

pub(crate) struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// `ureq` on desktop and Android, `fetch` in the browser. Async because
/// `fetch` is; the native one blocks on its own thread and is never pending.
pub(crate) trait Http {
    async fn send(&self, request: Request) -> Result<Response, String>;
}

// -------------------------------------------------------------------------
// Google's answers
// -------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Error {
    /// Google no longer accepts this sign-in — revoked, or the password
    /// changed. The only way on is to sign in again. (Only a refresh token can
    /// tell, so the web never sees this.)
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    SignedOut,
    /// The access token ran out; a new one is needed.
    Expired,
    /// The user closed the sign-in.
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignedOut => {
                f.write_str("Google ended this sign-in. Sign in again to keep syncing.")
            }
            Self::Expired => f.write_str("The sign-in needs renewing. Tap Sync now."),
            Self::Cancelled => f.write_str("Sign-in cancelled."),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

impl From<String> for Error {
    /// A transport failure: no connection, a timeout, a TLS error.
    fn from(why: String) -> Self {
        Self::Failed(format!("Could not reach Google: {why}"))
    }
}

/// An access token, good for about an hour.
#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub access: String,
    pub expires: Stamp,
}

impl Token {
    /// Still good for at least another minute?
    pub fn fresh(&self) -> bool {
        self.expires > progress::now() + 60_000
    }
}

/// What a token request comes back with: Google's token endpoint on desktop and
/// Android, Google's script on the web.
#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    /// A number from the token endpoint; Google's script has given strings.
    expires_in: serde_json::Value,
    refresh_token: Option<String>,
    #[serde(default)]
    scope: String,
}

/// Reads a token answer: the token, and a refresh token if one came with it.
///
/// Google's consent screen lets people untick the Drive permission and still
/// sign in. Without it there is nothing to sync, so that is an error here.
pub(crate) fn parse_token(body: &[u8]) -> Result<(Token, Option<String>), Error> {
    let answer: TokenAnswer = serde_json::from_slice(body)
        .map_err(|e| Error::Failed(format!("Google sent an unreadable token: {e}")))?;
    if !answer.scope.split(' ').any(|scope| scope == DRIVE_SCOPE) {
        return Err(Error::Failed(
            "WordTee needs its folder in your Google Drive to sync. Sign in again and leave that permission ticked."
                .into(),
        ));
    }
    let seconds = match &answer.expires_in {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
    .unwrap_or(3_600);
    Ok((
        Token {
            access: answer.access_token,
            expires: progress::now() + seconds * 1_000,
        },
        answer.refresh_token,
    ))
}

/// Who signed in.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(crate) struct Profile {
    pub email: String,
    #[serde(default)]
    pub name: String,
}

pub(crate) async fn user_info(
    http: &impl Http,
    endpoints: &Endpoints,
    token: &str,
) -> Result<Profile, Error> {
    let response = check(
        http.send(Request::get(endpoints.userinfo.clone(), token))
            .await?,
    )?;
    serde_json::from_slice(&response.body)
        .map_err(|e| Error::Failed(format!("Google sent an unreadable profile: {e}")))
}

/// Turns an HTTP status from Google's APIs into an [`Error`].
fn check(response: Response) -> Result<Response, Error> {
    match response.status {
        200..=299 => Ok(response),
        401 => Err(Error::Expired),
        // Google's APIs explain themselves in `error.message` — for instance
        // that the Drive API is not enabled for the OAuth client's project.
        status => {
            let message = serde_json::from_slice::<serde_json::Value>(&response.body)
                .ok()
                .and_then(|answer| answer["error"]["message"].as_str().map(str::to_owned))
                .unwrap_or_else(|| {
                    String::from_utf8_lossy(&response.body)
                        .chars()
                        .take(200)
                        .collect()
                });
            Err(Error::Failed(format!(
                "Google answered {status}: {message}"
            )))
        }
    }
}

// -------------------------------------------------------------------------
// Drive
// -------------------------------------------------------------------------

/// One sync: fetch Drive's copy, merge it into `local`, and put the result back
/// if Drive's copy was missing anything. Returns the merged progress.
pub(crate) async fn sync(
    http: &impl Http,
    endpoints: &Endpoints,
    token: &str,
    local: Progress,
) -> Result<Progress, Error> {
    let file = find(http, endpoints, token).await?;
    let remote = match &file {
        Some(id) => Some(download(http, endpoints, token, id).await?),
        None => None,
    };

    let mut merged = local;
    let stale = match remote {
        Some(mut remote) => {
            merged.merge(&remote);
            remote.merge(&merged)
        }
        None => true,
    };
    if stale {
        upload(http, endpoints, token, file.as_deref(), &merged).await?;
    }
    Ok(merged)
}

/// The id of the progress file, if there is one yet. Oldest first, so that if
/// two devices ever both created one, everyone settles on the same.
async fn find(
    http: &impl Http,
    endpoints: &Endpoints,
    token: &str,
) -> Result<Option<String>, Error> {
    #[derive(Deserialize)]
    struct Files {
        files: Vec<File>,
    }
    #[derive(Deserialize)]
    struct File {
        id: String,
    }

    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("spaces", "appDataFolder")
        .append_pair("q", &format!("name = '{FILE_NAME}' and trashed = false"))
        .append_pair("orderBy", "createdTime")
        .append_pair("fields", "files(id)")
        .finish();
    let url = format!("{}/files?{query}", endpoints.drive);
    let response = check(http.send(Request::get(url, token)).await?)?;
    let files: Files = serde_json::from_slice(&response.body)
        .map_err(|e| Error::Failed(format!("Google Drive sent an unreadable list: {e}")))?;
    Ok(files.files.into_iter().next().map(|file| file.id))
}

async fn download(
    http: &impl Http,
    endpoints: &Endpoints,
    token: &str,
    id: &str,
) -> Result<Progress, Error> {
    let url = format!("{}/files/{id}?alt=media", endpoints.drive);
    let response = check(http.send(Request::get(url, token)).await?)?;
    // Never merge, and so never upload over, a copy that did not read back:
    // a newer app version may have written it.
    serde_json::from_slice(&response.body).map_err(|e| {
        Error::Failed(format!(
            "The progress in Google Drive could not be read ({e}). Is this WordTee out of date?"
        ))
    })
}

/// Writes `progress` over the file `id`, or creates the file.
async fn upload(
    http: &impl Http,
    endpoints: &Endpoints,
    token: &str,
    id: Option<&str>,
    progress: &Progress,
) -> Result<(), Error> {
    let json = serde_json::to_vec(progress)
        .map_err(|e| Error::Failed(format!("Could not write the progress: {e}")))?;
    let request = match id {
        Some(id) => Request {
            method: "PATCH",
            url: format!("{}/files/{id}?uploadType=media", endpoints.upload),
            bearer: Some(token.to_owned()),
            body: Some(("application/json".into(), json)),
        },
        None => {
            let (content_type, body) = multipart(&json);
            Request {
                method: "POST",
                url: format!("{}/files?uploadType=multipart&fields=id", endpoints.upload),
                bearer: Some(token.to_owned()),
                body: Some((content_type, body)),
            }
        }
    };
    check(http.send(request).await?)?;
    Ok(())
}

/// Drive's create-with-content request: the file's metadata, then its bytes.
fn multipart(json: &[u8]) -> (String, Vec<u8>) {
    // Cannot occur in the JSON: it never holds a line starting with dashes.
    const BOUNDARY: &str = "wordtee-progress-7f3e9b";
    let metadata = format!(r#"{{"name":"{FILE_NAME}","parents":["appDataFolder"]}}"#);
    let mut body = format!(
        "--{BOUNDARY}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n\
         --{BOUNDARY}\r\nContent-Type: application/json\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(json);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    (format!("multipart/related; boundary={BOUNDARY}"), body)
}

// -------------------------------------------------------------------------
// the account, as the app holds it
// -------------------------------------------------------------------------

/// Kept in eframe's storage, next to the progress.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Account {
    email: String,
    name: String,
    /// Desktop and Android: gets a new access token without asking the user.
    /// The web has none; Google's script gives access tokens only.
    refresh_token: Option<String>,
    /// When Drive last had everything.
    synced: Option<Stamp>,
}

/// Results coming back from a sign-in or sync running elsewhere: a thread on
/// desktop and Android, a promise on the web.
pub(crate) enum Event {
    SignedIn {
        job: u64,
        profile: Profile,
        token: Token,
        refresh_token: Option<String>,
    },
    Synced {
        job: u64,
        merged: Box<Progress>,
        token: Token,
    },
    Failed {
        job: u64,
        error: Error,
    },
}

/// What a sync needs to get going.
pub(crate) struct SyncWith {
    /// The access token, if there is one still.
    pub token: Option<Token>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub refresh_token: Option<String>,
    /// For the web, to renew the token for the same account without asking
    /// which one.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub email: String,
}

/// What is running, if anything.
enum Job {
    SigningIn {
        id: u64,
        cancel: platform::Cancel,
    },
    /// `rev` is the progress revision the sync started from.
    Syncing {
        id: u64,
        rev: u64,
    },
}

impl Job {
    fn id(&self) -> u64 {
        match self {
            Self::SigningIn { id, .. } | Self::Syncing { id, .. } => *id,
        }
    }
}

/// What the Sync card shows.
pub enum Status {
    /// This build has no Google client ID.
    Unavailable,
    SignedOut {
        error: Option<String>,
    },
    SigningIn,
    SignedIn {
        name: String,
        email: String,
        syncing: bool,
        synced: Option<Stamp>,
        error: Option<String>,
        /// Sync waits for a tap on Sync now: on the web, once the access token
        /// has run out, a new one needs Google's popup.
        waiting: bool,
    },
}

/// Sign-in and sync, as the app sees them.
pub struct Google {
    account: Option<Account>,
    token: Option<Token>,
    job: Option<Job>,
    next_job: u64,
    error: Option<String>,
    events: mpsc::Sender<Event>,
    inbox: mpsc::Receiver<Event>,
    /// The progress revision Drive has everything up to, this session. `None`
    /// until the first sync, which runs as soon as it can.
    synced_rev: Option<u64>,
    /// `Context::input(|i| i.time)` of the last sync started.
    last_sync: f64,
}

impl Default for Google {
    fn default() -> Self {
        let (events, inbox) = mpsc::channel();
        Self {
            account: None,
            token: None,
            job: None,
            next_job: 0,
            error: None,
            events,
            inbox,
            synced_rev: None,
            last_sync: f64::NEG_INFINITY,
        }
    }
}

impl Google {
    /// Picks up the account saved last time, if any.
    pub fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        Self {
            account: storage.and_then(|s| eframe::get_value(s, STORAGE_KEY)),
            ..Self::default()
        }
    }

    pub fn save(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.account);
    }

    pub fn status(&self) -> Status {
        if !platform::available() {
            return Status::Unavailable;
        }
        if let Some(Job::SigningIn { .. }) = self.job {
            return Status::SigningIn;
        }
        match &self.account {
            None => Status::SignedOut {
                error: self.error.clone(),
            },
            Some(account) => Status::SignedIn {
                name: account.name.clone(),
                email: account.email.clone(),
                syncing: matches!(self.job, Some(Job::Syncing { .. })),
                synced: account.synced,
                error: self.error.clone(),
                waiting: !platform::can_sync_unattended(
                    self.token.as_ref(),
                    account.refresh_token.as_deref(),
                ),
            },
        }
    }

    /// Is there an account that changes here are synced to?
    pub fn signed_in(&self) -> bool {
        self.account.is_some()
    }

    /// Opens Google's sign-in. The result arrives through [`Self::update`].
    pub fn sign_in(&mut self, ctx: &egui::Context) {
        if self.account.is_some() || self.job.is_some() {
            return;
        }
        self.error = None;
        let id = self.job_id();
        match platform::sign_in(ctx, id, self.events.clone()) {
            Ok(cancel) => self.job = Some(Job::SigningIn { id, cancel }),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    /// Gives up on a sign-in in progress.
    pub fn cancel(&mut self) {
        if let Some(Job::SigningIn { cancel, .. }) = self.job.take() {
            cancel.cancel();
        }
    }

    /// Forgets the account and tells Google to drop its tokens. The progress on
    /// this device, and the copy in Drive, both stay.
    pub fn sign_out(&mut self) {
        self.cancel();
        if let Some(account) = self.account.take() {
            let token = account
                .refresh_token
                .or_else(|| self.token.as_ref().map(|t| t.access.clone()));
            if let Some(token) = token {
                platform::revoke(token);
            }
        }
        // A sync still running finishes unheard: its job is gone.
        self.job = None;
        self.token = None;
        self.error = None;
        self.synced_rev = None;
        self.last_sync = f64::NEG_INFINITY;
    }

    /// Syncs now. On the web this is also how a run-out token gets renewed:
    /// Google's popup needs a tap to be allowed to open.
    pub fn sync_now(&mut self, ctx: &egui::Context, progress: &Progress, now: f64) {
        if self.account.is_some() && self.job.is_none() {
            self.start_sync(ctx, progress, now);
        }
    }

    /// Once a frame: takes in finished work, and starts a sync when one is
    /// due — at startup, after signing in, and every [`SYNC_EVERY`] seconds
    /// while there are changes Drive has not seen.
    pub fn update(&mut self, ctx: &egui::Context, progress: &mut Progress, now: f64) {
        while let Ok(event) = self.inbox.try_recv() {
            self.handle(event, progress);
        }

        let Some(account) = &self.account else { return };
        if self.job.is_some()
            || !platform::available()
            || !platform::can_sync_unattended(self.token.as_ref(), account.refresh_token.as_deref())
            || self.synced_rev == Some(progress.revision())
        {
            return;
        }
        let wait = self.last_sync + SYNC_EVERY - now;
        if wait > 0.0 {
            ctx.request_repaint_after(Duration::from_secs_f64(wait));
        } else {
            self.start_sync(ctx, progress, now);
        }
    }

    fn start_sync(&mut self, ctx: &egui::Context, progress: &Progress, now: f64) {
        let id = self.job_id();
        let Some(account) = &self.account else { return };
        self.last_sync = now;
        self.job = Some(Job::Syncing {
            id,
            rev: progress.revision(),
        });
        platform::sync(
            ctx,
            id,
            self.events.clone(),
            SyncWith {
                token: self.token.clone(),
                refresh_token: account.refresh_token.clone(),
                email: account.email.clone(),
            },
            progress.clone(),
        );
    }

    fn handle(&mut self, event: Event, progress: &mut Progress) {
        let job = match &event {
            Event::SignedIn { job, .. } | Event::Synced { job, .. } | Event::Failed { job, .. } => {
                *job
            }
        };
        // A cancelled sign-in, or anything from before a sign-out.
        if self.job.as_ref().map(Job::id) != Some(job) {
            return;
        }
        let finished = self.job.take();

        match event {
            Event::SignedIn {
                profile,
                token,
                refresh_token,
                ..
            } => {
                self.account = Some(Account {
                    email: profile.email,
                    name: profile.name,
                    refresh_token,
                    synced: None,
                });
                self.token = Some(token);
                self.error = None;
                // Sync straight away.
                self.synced_rev = None;
                self.last_sync = f64::NEG_INFINITY;
            }
            Event::Synced { merged, token, .. } => {
                let Some(Job::Syncing { rev, .. }) = finished else {
                    return;
                };
                let changed_meanwhile = rev != progress.revision();
                progress.merge(&merged);
                // Drive now has everything up to where the sync started.
                // Changes made while it ran wait for the next one.
                self.synced_rev = Some(if changed_meanwhile {
                    rev
                } else {
                    progress.revision()
                });
                self.token = Some(token);
                self.error = None;
                if let Some(account) = &mut self.account {
                    account.synced = Some(progress::now());
                }
            }
            Event::Failed { error, .. } => {
                match error {
                    Error::SignedOut => {
                        self.account = None;
                        self.token = None;
                    }
                    Error::Expired => self.token = None,
                    _ => {}
                }
                self.error = (error != Error::Cancelled).then(|| error.to_string());
            }
        }
    }

    fn job_id(&mut self) -> u64 {
        self.next_job += 1;
        self.next_job
    }
}

#[cfg(test)]
mod tests;
