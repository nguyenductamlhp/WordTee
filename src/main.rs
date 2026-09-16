//! The `main` entry point, for desktop and for the web.
//!
//! Trunk compiles this same binary to WebAssembly, where `main` hands over to
//! `tcheckee::start_web` instead of opening a window. Android does not use
//! `main` at all; it enters through `android_main` in `src/android.rs`.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    eframe::run_native(
        tcheckee::APP_NAME,
        tcheckee::native_options(),
        Box::new(|cc| Ok(Box::new(tcheckee::TapCounterApp::new(cc)))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    tcheckee::start_web();
}
