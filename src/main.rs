//! The `main` entry point, for desktop and for the web.
//!
//! Trunk compiles this same binary to WebAssembly, where `main` hands over to
//! `wordtee::start_web` instead of opening a window. Android does not use
//! `main` at all; it enters through `android_main` in `src/android.rs`.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    eframe::run_native(
        wordtee::APP_NAME,
        wordtee::native_options(),
        Box::new(|cc| Ok(Box::new(wordtee::WordTeeApp::new(cc)))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    wordtee::start_web();
}
