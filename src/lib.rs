//! `wordtee` — tap anywhere on the screen to bump a counter.
//!
//! One app, three entry points:
//!
//! | Platform | Entry point |
//! | --- | --- |
//! | Desktop | `main` in `src/main.rs` |
//! | Android | [`android_main`](android) in `src/android.rs` (an exported `cdylib` symbol) |
//! | Web (Wasm) | [`start_web`] in `src/web.rs`, called from `main` |

mod app;
pub mod dict;
pub mod placement;
pub mod progress;
pub mod quiz;
pub mod rng;
pub mod search;
pub mod srs;
pub mod study;
pub mod ui;

pub use app::{APP_NAME, WordTeeApp};

#[cfg(target_os = "android")]
mod android;

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
pub use web::start as start_web;

/// Window options shared by the platforms that have windows.
#[cfg(not(target_arch = "wasm32"))]
pub fn native_options() -> eframe::NativeOptions {
    use eframe::egui;

    let viewport = egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_inner_size([420.0, 720.0])
        .with_min_inner_size([260.0, 320.0]);
    // Android takes its launcher icon from `android/res/` instead.
    #[cfg(not(target_os = "android"))]
    let viewport = viewport.with_icon(window_icon());

    eframe::NativeOptions {
        viewport,
        ..Default::default()
    }
}

/// The logo, for the window's title bar and the taskbar. Rendered from
/// `assets/logo.svg`; without it, eframe shows its own egui logo.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn window_icon() -> eframe::egui::IconData {
    let png = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icon.png"));
    eframe::icon_data::from_png_bytes(png).expect("assets/icon.png is a valid PNG")
}

#[cfg(all(test, not(target_arch = "wasm32"), not(target_os = "android")))]
mod tests {
    #[test]
    fn window_icon_decodes() {
        let icon = super::window_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
    }
}
