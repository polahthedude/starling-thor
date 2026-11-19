# Dual-screen handhelds

Starling Thor draws a second interface on the other display of devices like the AYN Thor. If
Android doesn't hand the app a second display, none of this runs and the game is the same as
upstream.

## Bottom screen

Laid out like the New 3DS edition, using vanilla's sprites: hotbar and coordinates along the top,
minimap on the left, and a 3x3 grid on the right (inventory, pause, chat, advancements, camera).
The top screen keeps the world and the status bars.

## Menus

Inventory, crafting with the recipe book, the creative inventory, anvil, pause and the world
screens are vanilla's own, scaled to fit the bottom screen in quarter steps. Hovering a recipe
shows it in a larger preview, and item tooltips show up next to it. The creative grid scrolls by
swiping and has a search keyboard. A hardware pad can drive the recipe book. The older 3DS-style
creative layout is still in the code behind `VANILLA_CREATIVE`.

## Launching

`LaunchActivity` starts the game on the top screen no matter which display you tapped the icon
on. The game activity is `singleTask`, so launching again returns to the running game. You can
also pin a control profile as a home-screen shortcut.

## Code

`crates/horizon-client/src/game/second_screen.rs` and `second_screen/`, with the surface coming
from `android/second_screen.rs`. The Java side is in `packaging/android/java/dev/horizon/android/`.

## Status

Only tried on the AYN Thor. Builds are experimental, see [dev builds](android-dev-builds.md).
