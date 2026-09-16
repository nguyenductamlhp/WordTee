//! Web entry point: runs the app in a `<canvas>` so it can be tried in a
//! browser without installing anything. See `web/index.html`.

use crate::WordTeeApp;
use eframe::wasm_bindgen::JsCast as _;
use eframe::web_sys;

/// `id` of the `<canvas>` the app draws into.
const CANVAS_ID: &str = "wordtee-canvas";

/// `id` of the placeholder shown until the Wasm module has booted.
const STATUS_ID: &str = "boot-status";

/// Boots the app. Returns immediately; eframe drives itself from the browser's
/// animation frames afterwards.
pub fn start() {
    // Route `log` to the devtools console and show panics there instead of
    // failing silently.
    eframe::WebLogger::init(log::LevelFilter::Info).ok();

    let document = web_sys::window()
        .and_then(|window| window.document())
        .expect("no DOM");

    let canvas = document
        .get_element_by_id(CANVAS_ID)
        .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .unwrap_or_else(|| panic!("the page has no <canvas id=\"{CANVAS_ID}\">"));

    wasm_bindgen_futures::spawn_local(async move {
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(WordTeeApp::new(cc)))),
            )
            .await;

        let status = document.get_element_by_id(STATUS_ID);
        match result {
            Ok(()) => {
                // Reveal the canvas by taking the placeholder away.
                if let Some(status) = status {
                    status.remove();
                }
            }
            Err(err) => {
                log::error!("failed to start eframe: {err:?}");
                if let Some(status) = status {
                    status.set_text_content(Some(
                        "Could not start the app. Your browser may not support WebGL 2.",
                    ));
                }
            }
        }
    });
}
