//! Android entry point.

use crate::{APP_NAME, WordTeeApp, native_options};
use android_activity::{AndroidApp, WindowManagerFlags};
use eframe::egui::{SafeAreaInsets, epaint::MarginF32};
use jni::{JValue, JavaVM, jni_sig, jni_str, objects::JObject, refs::Global, sys::jobject};

/// `android-activity` spawns a dedicated thread and calls this unmangled
/// `extern "Rust"` symbol once the `NativeActivity` has been created.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("wordtee"),
    );

    // Hide the status bar. The window still reaches under the navigation bar
    // on Android 15, which `SystemBars` keeps the UI out of the way of.
    app.set_window_flags(WindowManagerFlags::FULLSCREEN, WindowManagerFlags::empty());

    let system_bars = SystemBars::new(&app);

    let mut options = native_options();
    options.android_app = Some(app);

    if let Err(err) = eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(WordTeeApp::new(cc).with_system_bars(system_bars)))),
    ) {
        log::error!("eframe exited with an error: {err}");
    }
}

/// Where the status bar, the navigation bar and any camera cutout lie over the
/// window, for egui to keep its content out of.
///
/// An app that targets Android 15 is laid out under the system bars whatever
/// its theme asks for, so the navigation buttons sit on top of whatever egui
/// draws along the bottom of the screen. egui-winit only reports these insets
/// on iOS; here the window is asked for them.
pub struct SystemBars {
    app: AndroidApp,
    sdk: i32,
    /// So a failing call is logged once rather than every frame.
    warned: bool,
}

impl SystemBars {
    pub fn new(app: &AndroidApp) -> Self {
        Self {
            app: app.clone(),
            sdk: app.config().sdk_version(),
            warned: false,
        }
    }

    /// The insets in points, or `None` while the window is not laid out yet.
    ///
    /// Cheap enough to ask every frame, which also catches a rotation or a
    /// switch between gesture and button navigation as soon as it happens.
    pub fn insets(&mut self, pixels_per_point: f32) -> Option<SafeAreaInsets> {
        // `WindowInsets.getInsets` is API 30. Before Android 15 nothing lays a
        // window under the bars unless it asks to be, and this one does not.
        if self.sdk < 30 {
            return None;
        }

        let activity: jobject = self.app.activity_as_ptr().cast();
        let pixels = JavaVM::singleton().and_then(|vm| {
            vm.attach_current_thread(|env| -> jni::errors::Result<Option<[i32; 4]>> {
                // SAFETY: a global reference to the activity, which
                // android-activity keeps alive for as long as `app` is.
                let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&activity)? };
                let window = env
                    .call_method(
                        &activity,
                        jni_str!("getWindow"),
                        jni_sig!("()Landroid/view/Window;"),
                        &[],
                    )?
                    .l()?;
                let decor = env
                    .call_method(
                        &window,
                        jni_str!("getDecorView"),
                        jni_sig!("()Landroid/view/View;"),
                        &[],
                    )?
                    .l()?;
                let insets = env
                    .call_method(
                        &decor,
                        jni_str!("getRootWindowInsets"),
                        jni_sig!("()Landroid/view/WindowInsets;"),
                        &[],
                    )?
                    .l()?;
                if insets.is_null() {
                    return Ok(None);
                }

                // Only the bars on show: the status bar is hidden, and comes
                // back over the app for a moment when swiped down, as intended.
                let types = jni_str!("android/view/WindowInsets$Type");
                let mut mask = 0;
                for kind in [jni_str!("systemBars"), jni_str!("displayCutout")] {
                    mask |= env
                        .call_static_method(types, kind, jni_sig!("()I"), &[])?
                        .i()?;
                }
                let edges = env
                    .call_method(
                        &insets,
                        jni_str!("getInsets"),
                        jni_sig!("(I)Landroid/graphics/Insets;"),
                        &[JValue::Int(mask)],
                    )?
                    .l()?;

                let mut sides = [0; 4];
                let names = [
                    jni_str!("left"),
                    jni_str!("top"),
                    jni_str!("right"),
                    jni_str!("bottom"),
                ];
                for (side, name) in sides.iter_mut().zip(names) {
                    *side = env.get_field(&edges, name, jni_sig!("I"))?.i()?;
                }
                Ok(Some(sides))
            })
        });

        match pixels {
            Ok(pixels) => pixels.map(|[left, top, right, bottom]| {
                let points = |pixels: i32| pixels as f32 / pixels_per_point;
                SafeAreaInsets(MarginF32 {
                    left: points(left),
                    right: points(right),
                    top: points(top),
                    bottom: points(bottom),
                })
            }),
            Err(err) => {
                if !std::mem::replace(&mut self.warned, true) {
                    log::warn!("could not ask the window where the system bars are: {err}");
                }
                None
            }
        }
    }
}
