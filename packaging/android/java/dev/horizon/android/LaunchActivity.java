package dev.horizon.android;

/**
 * The launcher icon's activity, and every shortcut's (they target the old component's alias):
 * starts the game on the device's main display whichever display it was opened from, then goes.
 * A dual-screen handheld's other display stays the game's second screen. Android keeps an
 * activity on the display it started on, and the native activity can't start twice in one
 * process to move, so this window-less activity picks the display before the game starts.
 */
public final class LaunchActivity extends android.app.Activity {
    @Override protected void onCreate(android.os.Bundle saved) {
        super.onCreate(saved);
        android.content.Intent game = new android.content.Intent(getIntent())
                .setClass(this, HorizonActivity.class)
                .addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK);
        android.os.Bundle options = android.app.ActivityOptions.makeBasic()
                .setLaunchDisplayId(android.view.Display.DEFAULT_DISPLAY)
                .toBundle();
        try {
            startActivity(game, options);
        } catch (RuntimeException e) {
            // A display Android won't launch on: wherever it puts it.
            android.util.Log.w("Horizon", "Launch on the main display: " + e);
            startActivity(game);
        }
        finish();
    }
}
