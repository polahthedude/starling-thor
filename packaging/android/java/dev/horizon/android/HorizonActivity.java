package dev.horizon.android;

/**
 * SAF result callback, and the Surface of a second display (dual-screen handhelds), which needs a
 * Java SurfaceHolder.Callback. The native activity, UI, drawing and file operations stay Rust.
 */
public final class HorizonActivity extends android.app.NativeActivity {
    static { System.loadLibrary("horizon_android"); }
    private native void nativeDocumentResult(int request, int result, String uri);
    private static native void nativeSecondSurface(android.view.Surface surface, int width, int height);
    private static native void nativeSecondSurfaceGone();
    /** A finger on the second screen: 0 down, 1 moved, 2 up; in the surface's pixels. */
    private static native void nativeSecondTouch(int action, float x, float y);
    /** The system bars may be back: the game applies its screen preferences again. */
    private static native void nativeReapplyScreen();

    private boolean secondScreenWanted;
    /** Between onStart and onStop: the Presentation shows only while the game is visible. */
    private boolean started;
    private android.app.Presentation secondScreen;
    private android.hardware.display.DisplayManager.DisplayListener displays;

    /** Intent extra of a home-screen shortcut: the profile number to start straight into. */
    private static final String PROFILE_EXTRA = "horizon.profile";

    /** The profile this launch's shortcut asked for, or -1 for an ordinary launch. */
    public int shortcutProfile() {
        return getIntent() == null ? -1 : getIntent().getIntExtra(PROFILE_EXTRA, -1);
    }

    /**
     * Asks the home screen to pin a shortcut that starts {@code profile} directly. It targets the
     * launcher alias, so the game still opens on the main display. False when the home screen
     * can't pin shortcuts (the user is then asked nothing).
     */
    public boolean pinProfileShortcut(int profile, String label) {
        android.content.pm.ShortcutManager manager =
                getSystemService(android.content.pm.ShortcutManager.class);
        if (manager == null || !manager.isRequestPinShortcutSupported()) return false;
        android.content.Intent launch = new android.content.Intent(android.content.Intent.ACTION_MAIN)
                .setClassName(getPackageName(), "com.google.androidgamesdk.GameActivity")
                .putExtra(PROFILE_EXTRA, profile);
        android.content.pm.ShortcutInfo info = new android.content.pm.ShortcutInfo.Builder(
                this, "profile-" + profile)
                .setShortLabel(label)
                .setLongLabel(label)
                .setIcon(android.graphics.drawable.Icon.createWithResource(
                        this, getApplicationInfo().icon))
                .setIntent(launch)
                .build();
        return manager.requestPinShortcut(info, null);
    }

    /** From the game (any thread): show or hide the second screen, if there is one. */
    public void setSecondScreen(boolean wanted) {
        runOnUiThread(() -> {
            secondScreenWanted = wanted;
            android.hardware.display.DisplayManager manager =
                    getSystemService(android.hardware.display.DisplayManager.class);
            if (wanted && displays == null) {
                displays = new android.hardware.display.DisplayManager.DisplayListener() {
                    @Override public void onDisplayAdded(int id) { refreshSecondScreen(); }
                    @Override public void onDisplayRemoved(int id) { refreshSecondScreen(); }
                    @Override public void onDisplayChanged(int id) {}
                };
                manager.registerDisplayListener(displays, null);
            } else if (!wanted && displays != null) {
                manager.unregisterDisplayListener(displays);
                displays = null;
            }
            refreshSecondScreen();
        });
    }

    /** Presentation displays first, then any other public display than the game's. */
    private android.view.Display secondDisplay() {
        android.hardware.display.DisplayManager manager =
                getSystemService(android.hardware.display.DisplayManager.class);
        int own = getWindowManager().getDefaultDisplay().getDisplayId();
        for (String category : new String[] {
                android.hardware.display.DisplayManager.DISPLAY_CATEGORY_PRESENTATION, null}) {
            for (android.view.Display display : manager.getDisplays(category)) {
                if (display.getDisplayId() != own
                        && (display.getFlags() & android.view.Display.FLAG_PRIVATE) == 0) {
                    return display;
                }
            }
        }
        return null;
    }

    private void refreshSecondScreen() {
        android.view.Display target =
                secondScreenWanted && started && !isFinishing() ? secondDisplay() : null;
        if (secondScreen != null && (target == null
                || secondScreen.getDisplay().getDisplayId() != target.getDisplayId())) {
            secondScreen.dismiss(); // surfaceDestroyed tells the game first
            secondScreen = null;
        }
        if (target == null || secondScreen != null) return;
        android.app.Presentation shown = new android.app.Presentation(this, target);
        // Touches still arrive; keys and controllers stay with the game's window.
        shown.getWindow().addFlags(android.view.WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE);
        android.view.SurfaceView view = new android.view.SurfaceView(shown.getContext());
        view.getHolder().addCallback(new android.view.SurfaceHolder.Callback() {
            @Override public void surfaceCreated(android.view.SurfaceHolder holder) {}
            @Override public void surfaceChanged(android.view.SurfaceHolder holder, int format, int width, int height) {
                nativeSecondSurface(holder.getSurface(), width, height);
            }
            @Override public void surfaceDestroyed(android.view.SurfaceHolder holder) {
                nativeSecondSurfaceGone();
            }
        });
        // Fingers coming down (taps the game hit-tests), the first one moving (swipes) and
        // the last one lifting, in the surface's pixels.
        view.setOnTouchListener((touched, event) -> {
            int action = event.getActionMasked();
            int index = event.getActionIndex();
            if (action == android.view.MotionEvent.ACTION_DOWN
                    || action == android.view.MotionEvent.ACTION_POINTER_DOWN) {
                nativeSecondTouch(0, event.getX(index), event.getY(index));
            } else if (action == android.view.MotionEvent.ACTION_MOVE) {
                nativeSecondTouch(1, event.getX(0), event.getY(0));
            } else if (action == android.view.MotionEvent.ACTION_UP
                    || action == android.view.MotionEvent.ACTION_CANCEL) {
                nativeSecondTouch(2, event.getX(index), event.getY(index));
            }
            return true;
        });
        shown.setContentView(view);
        // The second screen is the game's too: no system bars over it.
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            android.view.Window window = shown.getWindow();
            window.setDecorFitsSystemWindows(false);
            android.view.WindowInsetsController bars = window.getInsetsController();
            if (bars != null) {
                bars.setSystemBarsBehavior(
                        android.view.WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
                bars.hide(android.view.WindowInsets.Type.systemBars());
            }
        }
        try {
            shown.show();
            secondScreen = shown;
        } catch (android.view.WindowManager.InvalidDisplayException e) {
            android.util.Log.w("Horizon", "Second screen: " + e);
        }
        // Showing a window on the other display brings the game window's bars back, some time
        // after: hide them again once it has settled.
        android.view.View decor = getWindow().getDecorView();
        decor.postDelayed(HorizonActivity::nativeReapplyScreen, 300);
        decor.postDelayed(HorizonActivity::nativeReapplyScreen, 1200);
    }

    /** Android shows the bars again when the window loses and regains focus (a dialog, the
     *  notification shade, the other display touched): the immersive mode goes back on. */
    @Override public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) nativeReapplyScreen();
    }

    @Override protected void onStart() {
        super.onStart();
        started = true;
        refreshSecondScreen();
    }

    @Override protected void onStop() {
        started = false;
        refreshSecondScreen();
        super.onStop();
    }

    @Override protected void onActivityResult(int request, int result, android.content.Intent data) {
        super.onActivityResult(request, result, data);
        if (request != 4101) return;
        nativeDocumentResult(request, result,
                data == null || data.getData() == null ? null : data.getData().toString());
    }
}
