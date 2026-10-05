//! Google sign-in in the browser, through Google Identity Services: its script,
//! loaded by `web/index.html`, opens Google's own popup and hands back an access
//! token, by way of the two small functions defined there.
//!
//! The web gets no refresh token. Once the hour-long access token runs out, the
//! next sync needs a tap on Sync now, because a popup only opens on a user's
//! action. For an account that has already agreed, Google closes the popup by
//! itself.

use std::sync::mpsc::Sender;

use eframe::egui;
use wasm_bindgen::{JsCast as _, prelude::*};
use wasm_bindgen_futures::{JsFuture, spawn_local};

use super::{Endpoints, Error, Event, Http, Request, Response, SCOPES, SyncWith, Token};
use crate::progress::Progress;

#[wasm_bindgen]
extern "C" {
    /// `wordteeGoogleToken` in web/index.html.
    #[wasm_bindgen(js_name = wordteeGoogleToken, catch)]
    fn request_token(
        client_id: &str,
        scope: &str,
        prompt: &str,
        hint: &str,
    ) -> Result<js_sys::Promise, JsValue>;

    /// `wordteeGoogleRevoke` in web/index.html.
    #[wasm_bindgen(js_name = wordteeGoogleRevoke, catch)]
    fn revoke_token(token: &str) -> Result<(), JsValue>;
}

/// The "Web application" OAuth client.
fn client_id() -> Option<&'static str> {
    option_env!("WORDTEE_GOOGLE_WEB_CLIENT_ID").filter(|id| !id.is_empty())
}

pub(super) fn available() -> bool {
    client_id().is_some()
}

/// Without a refresh token, only while the access token lasts.
pub(super) fn can_sync_unattended(token: Option<&Token>, _refresh_token: Option<&str>) -> bool {
    token.is_some_and(Token::fresh)
}

/// Google's popup cannot be closed from here; a cancelled sign-in's result is
/// simply ignored when it arrives.
pub(super) struct Cancel;

impl Cancel {
    pub fn cancel(&self) {}
}

pub(super) fn sign_in(
    ctx: &egui::Context,
    job: u64,
    events: Sender<Event>,
) -> Result<Cancel, Error> {
    let client_id =
        client_id().ok_or_else(|| Error::Failed("Google sign-in is not set up.".into()))?;
    let ctx = ctx.clone();
    spawn_local(async move {
        let run = async {
            let token = token(client_id, "select_account", "").await?;
            let profile = super::user_info(&WebHttp, &Endpoints::google(), &token.access).await?;
            Ok((profile, token))
        };
        let event = match run.await {
            Ok((profile, token)) => Event::SignedIn {
                job,
                profile,
                token,
                refresh_token: None,
            },
            Err(error) => Event::Failed { job, error },
        };
        let _ = events.send(event);
        ctx.request_repaint();
    });
    Ok(Cancel)
}

pub(super) fn sync(
    ctx: &egui::Context,
    job: u64,
    events: Sender<Event>,
    with: SyncWith,
    local: Progress,
) {
    let ctx = ctx.clone();
    spawn_local(async move {
        let run = async {
            let token = match with.token.filter(Token::fresh) {
                Some(token) => token,
                // Same account, no chooser.
                None => {
                    let client_id = client_id().ok_or(Error::Expired)?;
                    token(client_id, "", &with.email).await?
                }
            };
            let merged = super::sync(&WebHttp, &Endpoints::google(), &token.access, local).await?;
            Ok((merged, token))
        };
        let event = match run.await {
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

pub(super) fn revoke(token: String) {
    if let Err(why) = revoke_token(&token) {
        log::warn!("could not revoke the Google sign-in: {}", describe(&why));
    }
}

/// Asks Google's script for an access token.
async fn token(client_id: &str, prompt: &str, hint: &str) -> Result<Token, Error> {
    let promise = request_token(client_id, SCOPES, prompt, hint).map_err(|e| {
        log::error!("wordteeGoogleToken: {}", describe(&e));
        Error::Failed("This page cannot sign in to Google: its sign-in script is missing.".into())
    })?;
    let answer = JsFuture::from(promise).await.map_err(|e| match describe(&e).as_str() {
        // Closed the popup, or said no.
        "popup_closed" | "access_denied" => Error::Cancelled,
        "popup_failed_to_open" => Error::Failed(
            "The browser blocked Google's sign-in window. Allow pop-ups for this site and try again."
                .into(),
        ),
        "not_loaded" => Error::Failed(
            "Google's sign-in did not load. Check the connection, or an ad blocker, and try again."
                .into(),
        ),
        other => Error::Failed(format!("Google sign-in failed: {other}")),
    })?;
    let json = js_sys::JSON::stringify(&answer)
        .ok()
        .and_then(|s| s.as_string())
        .unwrap_or_default();
    Ok(super::parse_token(json.as_bytes())?.0)
}

/// An `Error`'s message, or whatever else was thrown, as text.
fn describe(value: &JsValue) -> String {
    value
        .dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| value.as_string())
        .unwrap_or_else(|| format!("{value:?}"))
}

/// `fetch`.
struct WebHttp;

impl Http for WebHttp {
    async fn send(&self, request: Request) -> Result<Response, String> {
        let js = |e: JsValue| describe(&e);
        let headers = web_sys::Headers::new().map_err(js)?;
        if let Some(token) = &request.bearer {
            headers
                .set("Authorization", &format!("Bearer {token}"))
                .map_err(js)?;
        }
        let init = web_sys::RequestInit::new();
        init.set_method(request.method);
        if let Some((content_type, bytes)) = &request.body {
            headers.set("Content-Type", content_type).map_err(js)?;
            init.set_body(&js_sys::Uint8Array::from(bytes.as_slice()));
        }
        init.set_headers(&headers);
        let fetch_request =
            web_sys::Request::new_with_str_and_init(&request.url, &init).map_err(js)?;

        let window = web_sys::window().ok_or("no window")?;
        let response: web_sys::Response = JsFuture::from(window.fetch_with_request(&fetch_request))
            .await
            .map_err(js)?
            .dyn_into()
            .map_err(js)?;
        let buffer = JsFuture::from(response.array_buffer().map_err(js)?)
            .await
            .map_err(js)?;
        Ok(Response {
            status: response.status(),
            body: js_sys::Uint8Array::new(&buffer).to_vec(),
        })
    }
}
