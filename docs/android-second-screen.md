# Dual-screen handhelds (Starling Thor)

This fork draws a second interface on the other display of dual-screen Android
handhelds such as the AYN Thor. Single-screen devices are unaffected: the lower
screen only exists while Android gives the app a second display surface.

## Layout

The lower screen follows New 3DS Edition's touch screen, in vanilla's sprites:

- hotbar along the top, coordinates beneath it;
- the minimap at the left;
- a 3x3 button grid at the right: inventory, pause, chat, advancements, camera.

The main display keeps the world and the status bars.

## Menus

Vanilla's own screens are drawn on the lower screen, fitted in quarter-size
steps: inventory, crafting with the recipe book (hover shows the recipe in a
showcase), creative inventory (swipe scrolling with momentum, search with an
on-screen keyboard), anvil, pause menu and the world screens. Item tooltips
appear beside the showcase. Hardware pad input drives the recipe book. The
3DS-style creative layout stays behind `VANILLA_CREATIVE`.

## Launching

`LaunchActivity` starts the game on the main display whichever display the
launcher icon or a shortcut was opened from. The game activity is `singleTask`,
so a second launch brings the running game back. A control profile can be pinned
as a home-screen shortcut that starts the game directly.

## Source

`crates/horizon-client/src/game/second_screen.rs` and `second_screen/`;
the surface comes from `android/second_screen.rs`;
the Java side is `packaging/android/java/dev/horizon/android/`.

## Status

Developed on and for the AYN Thor. Other dual-screen devices are untested.
Builds are experimental; see [dev builds](android-dev-builds.md).
