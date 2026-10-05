//! Sync and sign-in against stand-ins for Google: an in-memory Drive for the
//! sync itself, and a local HTTP server for the desktop sign-in.

use std::cell::{Cell, RefCell};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;

use super::native::{Client, Flow, NativeHttp, Redirect, block_on, challenge, refresh};
use super::*;
use crate::progress::{Source, State};

// -------------------------------------------------------------------------
// sync, against an in-memory Drive
// -------------------------------------------------------------------------

/// Drive's app data folder holding at most the one file, as the API shows it.
struct FakeDrive {
    file: RefCell<Option<Vec<u8>>>,
    uploads: Cell<u32>,
}

const TOKEN: &str = "good-token";
const FILE_ID: &str = "file-1";

impl FakeDrive {
    fn new() -> Self {
        Self {
            file: RefCell::new(None),
            uploads: Cell::new(0),
        }
    }

    fn stored(&self) -> Progress {
        serde_json::from_slice(self.file.borrow().as_ref().expect("a file")).expect("valid JSON")
    }
}

fn reply(status: u16, body: &str) -> Response {
    Response {
        status,
        body: body.as_bytes().to_vec(),
    }
}

impl Http for FakeDrive {
    async fn send(&self, request: Request) -> Result<Response, String> {
        if request.bearer.as_deref() != Some(TOKEN) {
            return Ok(reply(401, "{}"));
        }
        let endpoints = Endpoints::google();
        let url = &request.url;
        let exists = self.file.borrow().is_some();
        Ok(match request.method {
            "GET" if url.starts_with(&format!("{}/files?", endpoints.drive)) => {
                assert!(url.contains("spaces=appDataFolder"), "{url}");
                if exists {
                    reply(200, &format!(r#"{{"files":[{{"id":"{FILE_ID}"}}]}}"#))
                } else {
                    reply(200, r#"{"files":[]}"#)
                }
            }
            "GET" if *url == format!("{}/files/{FILE_ID}?alt=media", endpoints.drive) => Response {
                status: 200,
                body: self.file.borrow().clone().expect("listed, so it exists"),
            },
            "POST" => {
                assert!(!exists, "creates a second file");
                let (content_type, body) = request.body.expect("a body");
                let boundary = content_type
                    .strip_prefix("multipart/related; boundary=")
                    .expect("multipart");
                let body = String::from_utf8(body).expect("text");
                let parts: Vec<&str> = body.split(&format!("--{boundary}")).collect();
                assert_eq!(parts.len(), 4, "preamble, metadata, content, end: {body}");
                assert!(parts[1].contains(r#""parents":["appDataFolder"]"#));
                let content = parts[2].split_once("\r\n\r\n").expect("headers").1;
                *self.file.borrow_mut() =
                    Some(content.trim_end_matches("\r\n").as_bytes().to_vec());
                self.uploads.set(self.uploads.get() + 1);
                reply(200, &format!(r#"{{"id":"{FILE_ID}"}}"#))
            }
            "PATCH" => {
                assert_eq!(
                    *url,
                    format!("{}/files/{FILE_ID}?uploadType=media", endpoints.upload)
                );
                *self.file.borrow_mut() = Some(request.body.expect("a body").1);
                self.uploads.set(self.uploads.get() + 1);
                reply(200, "{}")
            }
            _ => panic!("unexpected {} {url}", request.method),
        })
    }
}

fn sync_with(drive: &FakeDrive, token: &str, local: &Progress) -> Result<Progress, Error> {
    block_on(sync(drive, &Endpoints::google(), token, local.clone()))
}

#[test]
fn two_devices_end_up_with_everything() {
    let drive = FakeDrive::new();
    let mut phone = Progress::default();
    phone.start_learning(1, Source::Manual, 100);
    let mut laptop = Progress::default();
    laptop.set_state(2, State::Known, Source::Manual, 100);

    // The first sync creates the file.
    phone = sync_with(&drive, TOKEN, &phone).expect("syncs");
    assert_eq!(drive.uploads.get(), 1);
    assert!(drive.stored().card(1).is_some());

    // The laptop gets the phone's card, and Drive the laptop's.
    laptop = sync_with(&drive, TOKEN, &laptop).expect("syncs");
    assert!(laptop.card(1).is_some() && laptop.card(2).is_some());
    assert_eq!(drive.uploads.get(), 2);

    // The phone catches up, with nothing new to send.
    phone = sync_with(&drive, TOKEN, &phone).expect("syncs");
    assert!(phone.card(2).is_some());
    assert_eq!(drive.uploads.get(), 2, "uploaded a copy Drive already had");
}

#[test]
fn a_rejected_token_is_reported_as_expired() {
    let drive = FakeDrive::new();
    let error = sync_with(&drive, "old-token", &Progress::default()).unwrap_err();
    assert_eq!(error, Error::Expired);
}

#[test]
fn googles_own_explanation_is_shown() {
    let answer = br#"{"error":{"code":403,"message":"Google Drive API has not been used in project 1 before or it is disabled.","status":"PERMISSION_DENIED"}}"#;
    let error = check(Response {
        status: 403,
        body: answer.to_vec(),
    })
    .err()
    .unwrap();
    assert_eq!(
        error.to_string(),
        "Google answered 403: Google Drive API has not been used in project 1 before or it is disabled."
    );
}

#[test]
fn an_unreadable_copy_is_never_overwritten() {
    let drive = FakeDrive::new();
    *drive.file.borrow_mut() = Some(b"{\"from\": \"a newer WordTee\"".to_vec());
    let error = sync_with(&drive, TOKEN, &Progress::default()).unwrap_err();
    assert!(matches!(error, Error::Failed(_)), "{error:?}");
    assert_eq!(drive.uploads.get(), 0);
}

#[test]
fn a_token_without_drive_access_is_refused() {
    let full = br#"{"access_token":"t","expires_in":"3599","scope":"openid https://www.googleapis.com/auth/drive.appdata email"}"#;
    let (token, refresh_token) = parse_token(full).expect("accepted");
    assert_eq!(token.access, "t");
    assert!(token.fresh() && refresh_token.is_none());

    let unticked = br#"{"access_token":"t","expires_in":3599,"scope":"openid email profile"}"#;
    assert!(matches!(parse_token(unticked), Err(Error::Failed(_))));
}

// -------------------------------------------------------------------------
// the desktop and Android sign-in
// -------------------------------------------------------------------------

#[test]
fn pkce_challenge_matches_rfc_7636() {
    // Appendix B of the RFC.
    assert_eq!(
        challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn redirects_are_read_from_the_request_line() {
    assert_eq!(
        Redirect::parse("GET /?state=s1&code=4%2F0Ab_x&scope=email HTTP/1.1\r\n"),
        Some(Redirect {
            code: Some("4/0Ab_x".into()),
            state: Some("s1".into()),
            error: None,
        })
    );
    assert_eq!(
        Redirect::parse("GET /?error=access_denied&state=s1 HTTP/1.1").and_then(|r| r.error),
        Some("access_denied".into())
    );
    assert_eq!(Redirect::parse("GET /favicon.ico HTTP/1.1"), None);
    assert_eq!(Redirect::parse("POST /?code=x HTTP/1.1"), None);
    assert_eq!(Redirect::parse(""), None);
}

/// A stand-in for Google's token and userinfo endpoints, on a local port.
/// Answers `requests` requests, then stops.
fn fake_google(requests: usize) -> (Endpoints, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
    let base = format!("http://{}", listener.local_addr().unwrap());
    let endpoints = Endpoints {
        auth: format!("{base}/auth"),
        token: format!("{base}/token"),
        revoke: format!("{base}/revoke"),
        userinfo: format!("{base}/userinfo"),
        drive: format!("{base}/drive"),
        upload: format!("{base}/upload"),
    };
    let server = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for stream in listener.incoming().take(requests) {
            let mut stream = stream.expect("connection");
            let (line, body) = read_request(&mut stream);
            let fields: std::collections::HashMap<String, String> =
                form_urlencoded::parse(body.as_bytes())
                    .into_owned()
                    .collect();
            let (status, answer) = if line.starts_with("POST /token") {
                match fields.get("grant_type").map(String::as_str) {
                    Some("authorization_code") if fields["code"] == "the-code" => {
                        assert_eq!(fields["client_secret"], "secret");
                        assert_eq!(fields["code_verifier"].len(), 43, "32 bytes of base64");
                        (
                            200,
                            r#"{"access_token":"access-1","expires_in":3599,"refresh_token":"refresh-1","scope":"openid email profile https://www.googleapis.com/auth/drive.appdata"}"#,
                        )
                    }
                    Some("refresh_token") if fields["refresh_token"] == "refresh-1" => (
                        200,
                        r#"{"access_token":"access-2","expires_in":3599,"scope":"https://www.googleapis.com/auth/drive.appdata"}"#,
                    ),
                    Some("refresh_token") => (
                        400,
                        r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#,
                    ),
                    _ => (400, r#"{"error":"invalid_request"}"#),
                }
            } else if line.starts_with("GET /userinfo") {
                (
                    200,
                    r#"{"sub":"1","email":"me@example.com","name":"Me","email_verified":true}"#,
                )
            } else {
                (404, "{}")
            };
            write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            )
            .unwrap();
            seen.push(line);
        }
        seen
    });
    (endpoints, server)
}

/// The request line and the body of one HTTP request.
fn read_request(stream: &mut TcpStream) -> (String, String) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        if header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    (line.trim_end().to_owned(), String::from_utf8(body).unwrap())
}

/// The browser coming back to the app: one GET, and the page it gets.
fn visit(url: &str) -> String {
    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = if path.is_empty() { "/" } else { path };
    let mut stream = TcpStream::connect(host).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
    let mut page = String::new();
    stream.read_to_string(&mut page).unwrap();
    page
}

#[test]
fn signing_in_on_desktop() {
    let (endpoints, google) = fake_google(2);
    let client = Client {
        id: "client.apps.googleusercontent.com",
        secret: "secret",
    };
    let flow = Flow::start(&client, &endpoints).expect("starts");

    // What the browser is sent to.
    let (auth, query) = flow.url.split_once('?').unwrap();
    assert_eq!(auth, endpoints.auth);
    let query: std::collections::HashMap<String, String> = form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    assert_eq!(query["client_id"], client.id);
    assert_eq!(query["scope"], SCOPES);
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["access_type"], "offline");
    let redirect = query["redirect_uri"].clone();
    assert!(redirect.starts_with("http://127.0.0.1:"), "{redirect}");
    let state = query["state"].clone();

    // The browser, meanwhile: a favicon, a visit with someone else's state,
    // then Google's redirect.
    let browser = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let favicon = visit(&format!("{redirect}/favicon.ico"));
        let stranger = visit(&format!("{redirect}/?code=stolen&state=not-it"));
        let landed = visit(&format!(
            "{redirect}/?code=the-code&state={state}&scope=email"
        ));
        (favicon, stranger, landed)
    });

    let (profile, token, refresh_token) = block_on(flow.finish(
        &NativeHttp::new(),
        &client,
        &endpoints,
        &AtomicBool::new(false),
    ))
    .expect("signs in");
    assert_eq!(profile.email, "me@example.com");
    assert_eq!(profile.name, "Me");
    assert_eq!(token.access, "access-1");
    assert_eq!(refresh_token.as_deref(), Some("refresh-1"));

    let (favicon, stranger, landed) = browser.join().unwrap();
    assert!(favicon.starts_with("HTTP/1.1 404"), "{favicon}");
    assert!(stranger.starts_with("HTTP/1.1 400"), "{stranger}");
    assert!(
        landed.starts_with("HTTP/1.1 200") && landed.contains("Signed in"),
        "{landed}"
    );
    assert_eq!(
        google.join().unwrap(),
        ["POST /token HTTP/1.1", "GET /userinfo HTTP/1.1"]
    );
}

#[test]
fn saying_no_to_google_cancels() {
    let (endpoints, _) = fake_google(0);
    let client = Client {
        id: "c",
        secret: "s",
    };
    let flow = Flow::start(&client, &endpoints).expect("starts");
    let query: std::collections::HashMap<String, String> =
        form_urlencoded::parse(flow.url.split_once('?').unwrap().1.as_bytes())
            .into_owned()
            .collect();
    let back = format!(
        "{}/?error=access_denied&state={}",
        query["redirect_uri"], query["state"]
    );
    let browser = std::thread::spawn(move || visit(&back));
    let result = block_on(flow.finish(
        &NativeHttp::new(),
        &client,
        &endpoints,
        &AtomicBool::new(false),
    ));
    assert_eq!(result.unwrap_err(), Error::Cancelled);
    assert!(browser.join().unwrap().contains("Sign-in cancelled"));
}

#[test]
fn cancelling_stops_the_wait() {
    let (endpoints, _) = fake_google(0);
    let flow = Flow::start(
        &Client {
            id: "c",
            secret: "s",
        },
        &endpoints,
    )
    .expect("starts");
    let result = block_on(flow.finish(
        &NativeHttp::new(),
        &Client {
            id: "c",
            secret: "s",
        },
        &endpoints,
        &AtomicBool::new(true),
    ));
    assert_eq!(result.unwrap_err(), Error::Cancelled);
}

#[test]
fn a_dead_refresh_token_signs_out() {
    let (endpoints, google) = fake_google(2);
    let client = Client {
        id: "c",
        secret: "secret",
    };
    let http = NativeHttp::new();
    let token = block_on(refresh(&http, &client, &endpoints, "refresh-1")).expect("refreshes");
    assert_eq!(token.access, "access-2");
    let error = block_on(refresh(&http, &client, &endpoints, "revoked")).unwrap_err();
    assert_eq!(error, Error::SignedOut);
    google.join().unwrap();
}

// -------------------------------------------------------------------------
// the account, as the app holds it
// -------------------------------------------------------------------------

fn signed_in() -> Google {
    Google {
        account: Some(Account {
            email: "me@example.com".into(),
            name: "Me".into(),
            refresh_token: None,
            synced: None,
        }),
        token: Some(Token {
            access: TOKEN.into(),
            expires: progress::now() + 3_600_000,
        }),
        ..Google::default()
    }
}

#[test]
fn a_finished_sync_is_merged_in() {
    let ctx = egui::Context::default();
    let mut google = signed_in();
    let mut progress = Progress::default();
    google.job = Some(Job::Syncing {
        id: 7,
        rev: progress.revision(),
    });
    let mut from_drive = Progress::default();
    from_drive.start_learning(5, Source::Manual, 100);
    let token = google.token.clone().unwrap();
    google
        .events
        .send(Event::Synced {
            job: 7,
            merged: Box::new(from_drive),
            token,
        })
        .unwrap();

    google.update(&ctx, &mut progress, 0.0);
    assert!(progress.card(5).is_some());
    assert!(google.job.is_none());
    assert_eq!(
        google.synced_rev,
        Some(progress.revision()),
        "all of it is in Drive"
    );
    assert!(google.account.as_ref().unwrap().synced.is_some());
}

#[test]
fn changes_made_during_a_sync_wait_for_the_next() {
    let ctx = egui::Context::default();
    let mut google = signed_in();
    let mut progress = Progress::default();
    let started = progress.revision();
    google.job = Some(Job::Syncing {
        id: 1,
        rev: started,
    });
    google.last_sync = 0.0;
    progress.start_learning(9, Source::Manual, 100);
    let token = google.token.clone().unwrap();
    google
        .events
        .send(Event::Synced {
            job: 1,
            merged: Box::new(Progress::default()),
            token,
        })
        .unwrap();

    google.update(&ctx, &mut progress, 0.0);
    assert_eq!(google.synced_rev, Some(started));
    assert!(progress.card(9).is_some());
}

#[test]
fn results_of_abandoned_work_are_ignored() {
    let ctx = egui::Context::default();
    let mut google = signed_in();
    let mut progress = Progress::default();
    google.synced_rev = Some(progress.revision());
    let mut stale = Progress::default();
    stale.start_learning(3, Source::Manual, 100);
    let token = google.token.clone().unwrap();
    google
        .events
        .send(Event::Synced {
            job: 99,
            merged: Box::new(stale),
            token,
        })
        .unwrap();

    google.update(&ctx, &mut progress, 0.0);
    assert!(progress.card(3).is_none());
}

#[test]
fn google_ending_the_sign_in_signs_out() {
    let ctx = egui::Context::default();
    let mut google = signed_in();
    let mut progress = Progress::default();
    google.job = Some(Job::Syncing { id: 2, rev: 0 });
    google
        .events
        .send(Event::Failed {
            job: 2,
            error: Error::SignedOut,
        })
        .unwrap();

    google.update(&ctx, &mut progress, 0.0);
    assert!(google.account.is_none() && google.token.is_none());
    assert!(google.error.is_some());
}
