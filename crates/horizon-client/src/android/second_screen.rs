//! The second display of dual-screen handhelds (AYN Thor...). `HorizonActivity` shows a
//! Presentation there while the setting is on and hands its Surface over; the game's thread
//! draws on it (`game/second_screen.rs`). Surfaces arrive on Android's UI thread: a destroyed one
//! waits until the game has dropped what it drew on it, as Android requires.
use jni::{
    JNIEnv,
    objects::{JClass, JObject, JValue},
    sys::jint,
};
use std::{
    ffi::c_void,
    ptr::NonNull,
    sync::{Condvar, Mutex},
    time::Duration,
};

unsafe extern "C" {
    fn ANativeWindow_fromSurface(
        env: *mut jni::sys::JNIEnv,
        surface: jni::sys::jobject,
    ) -> *mut c_void;
    fn ANativeWindow_release(window: *mut c_void);
}

/// A reference to the second display's native window, released on drop.
pub struct NativeWindow(NonNull<c_void>);
// SAFETY: an ANativeWindow reference may be used and released from any thread.
unsafe impl Send for NativeWindow {}
impl Drop for NativeWindow {
    fn drop(&mut self) {
        unsafe { ANativeWindow_release(self.0.as_ptr()) }
    }
}
impl NativeWindow {
    /// A wgpu target on this window. The window must outlive the surface made from it.
    pub fn target(&self) -> Result<wgpu::SurfaceTargetUnsafe, String> {
        use winit::raw_window_handle::{
            AndroidDisplayHandle, AndroidNdkWindowHandle, DisplayHandle, HandleError,
            HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle,
        };
        struct Handles(NonNull<c_void>);
        impl HasWindowHandle for Handles {
            fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
                let raw = RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(self.0));
                Ok(unsafe { WindowHandle::borrow_raw(raw) })
            }
        }
        impl HasDisplayHandle for Handles {
            fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
                let raw = RawDisplayHandle::Android(AndroidDisplayHandle::new());
                Ok(unsafe { DisplayHandle::borrow_raw(raw) })
            }
        }
        unsafe { wgpu::SurfaceTargetUnsafe::from_window(&Handles(self.0)) }
            .map_err(|e| e.to_string())
    }
}

pub enum Change {
    /// A surface to draw on (new, or the old one at a new size), in physical pixels.
    Open(NativeWindow, [u32; 2]),
    /// The surface is going away: drop what draws on it, then call `closed`.
    Close,
}

struct State {
    pending: Option<Change>,
    /// The game holds a surface on the window Android last gave.
    drawing: bool,
}
static STATE: Mutex<State> = Mutex::new(State {
    pending: None,
    drawing: false,
});
static RELEASED: Condvar = Condvar::new();

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// What changed since the last frame. Taking `Open` means the game now draws on that window.
pub fn take_change() -> Option<Change> {
    let mut state = state();
    let change = state.pending.take();
    if matches!(change, Some(Change::Open(..))) {
        state.drawing = true;
    }
    change
}

/// The game no longer draws on the window (after `Change::Close`, or when opening failed).
pub fn closed() {
    state().drawing = false;
    RELEASED.notify_all();
}

/// The game draws on the window it kept again (a new game's surface on it).
pub fn drawing() {
    state().drawing = true;
}

/// Android showed the system bars again (the activity's window regained focus, the
/// Presentation came up): the game's screen preferences once more.
#[unsafe(export_name = "Java_dev_horizon_android_HorizonActivity_nativeReapplyScreen")]
extern "system" fn reapply_screen(_env: JNIEnv<'_>, _class: JClass<'_>) {
    crate::native_ime::reapply_screen();
}

/// Show (or hide) the Presentation on the second display, if the device has one.
pub fn enable(on: bool) {
    crate::platform::with_context(|env, activity| {
        env.call_method(
            activity,
            "setSecondScreen",
            "(Z)V",
            &[JValue::Bool(on.into())],
        )?;
        Ok(())
    });
}

#[unsafe(export_name = "Java_dev_horizon_android_HorizonActivity_nativeSecondSurface")]
extern "system" fn second_surface(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    surface: JObject<'_>,
    width: jint,
    height: jint,
) {
    let window = unsafe { ANativeWindow_fromSurface(env.get_raw(), surface.as_raw()) };
    let Some(window) = NonNull::new(window).map(NativeWindow) else {
        log::warn!("Second screen: Android gave no native window");
        return;
    };
    let size = [width.max(1) as u32, height.max(1) as u32];
    state().pending = Some(Change::Open(window, size));
}

/// A finger on the second screen, in its physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Touch {
    Down([f32; 2]),
    Move([f32; 2]),
    Up([f32; 2]),
}

/// Touches since the last frame.
static TOUCHES: Mutex<Vec<Touch>> = Mutex::new(Vec::new());

pub fn take_touches() -> Vec<Touch> {
    std::mem::take(&mut *TOUCHES.lock().unwrap_or_else(|e| e.into_inner()))
}

#[unsafe(export_name = "Java_dev_horizon_android_HorizonActivity_nativeSecondTouch")]
extern "system" fn second_touch(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    action: jint,
    x: f32,
    y: f32,
) {
    let touch = match action {
        0 => Touch::Down([x, y]),
        1 => Touch::Move([x, y]),
        _ => Touch::Up([x, y]),
    };
    let mut touches = TOUCHES.lock().unwrap_or_else(|e| e.into_inner());
    // A move replaces the move before it; bounded while the game isn't taking them
    // (the launcher, a long frame).
    if let (Touch::Move(_), Some(Touch::Move(_))) = (touch, touches.last()) {
        touches.pop();
    }
    if touches.len() < 64 {
        touches.push(touch);
    }
}

#[unsafe(export_name = "Java_dev_horizon_android_HorizonActivity_nativeSecondSurfaceGone")]
extern "system" fn second_surface_gone(_env: JNIEnv<'_>, _class: JClass<'_>) {
    let mut state = state();
    state.pending = Some(Change::Close);
    // Bounded: a game stuck in a long frame (or gone back to the launcher) must not freeze
    // Android's UI thread into an ANR. The window reference keeps the memory valid meanwhile.
    let (_state, timeout) = RELEASED
        .wait_timeout_while(state, Duration::from_millis(500), |s| s.drawing)
        .unwrap_or_else(|e| e.into_inner());
    if timeout.timed_out() {
        log::warn!("Second screen: surface destroyed while the game still drew on it");
    }
}
