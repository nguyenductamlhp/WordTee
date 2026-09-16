//! Android entry point.

use crate::{APP_NAME, TapCounterApp, native_options};
use android_activity::{AndroidApp, WindowManagerFlags};

/// `android-activity` spawns a dedicated thread and calls this unmangled
/// `extern "Rust"` symbol once the `NativeActivity` has been created.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("tcheckee"),
    );

    // Draw behind the status/navigation bars so the whole screen is tappable.
    app.set_window_flags(WindowManagerFlags::FULLSCREEN, WindowManagerFlags::empty());

    let mut options = native_options();
    options.android_app = Some(app);

    if let Err(err) = eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(TapCounterApp::new(cc)))),
    ) {
        log::error!("eframe exited with an error: {err}");
    }
}
