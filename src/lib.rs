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

    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_NAME)
            .with_inner_size([420.0, 720.0])
            .with_min_inner_size([260.0, 320.0]),
        ..Default::default()
    }
}
