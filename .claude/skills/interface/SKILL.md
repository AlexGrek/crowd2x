---
name: interface
description: crowd2x screens and interface - AppState, DespawnOnExit, the OnEnter-before-Startup ordering trap, bevy_ui on the upscale camera and UiScale, the one-highlight focus system for mouse, keyboard and gamepad (Focusable, Scope, KeyboardCapture, Activated/Cancelled, modals and GlobalZIndex), the on-screen keyboard, the main menu and the map browser. Use when adding or changing a screen, menu, button, dialog, text entry, navigation or focus behaviour, or the saved-maps browser.
---

# Screens and interface

`AppState` (`src/state.rs`) is `MainMenu`, `Maps` (the map browser), `Editor` or `Game`.
Screen entities carry `DespawnOnExit(..)` instead of hand-written teardown, which is also
why drawing a map takes the state it is being drawn for: the editor and the game draw the
same map from the same palettes and each takes its own sprites away on the way out.

**Two ordering traps live here.** `bevy_state` inserts `StateTransition` *before*
`PreStartup`, so the initial `OnEnter` runs before **every** `Startup` system — an
`OnEnter` system cannot see the camera or `PixelCanvas` that `render::setup_pipeline`
creates, and `Res<PixelCanvas>` there would panic. Anything a screen needs *before* its
`OnEnter` (the QA harness's fixture maps, `CROWD2X_MAP`) therefore has to be done in
`Plugin::build`, not in `Startup`.

The menu and HUD are stock `bevy_ui` (`Node`, `Button`, `Interaction`), which sidesteps
that: root nodes bind to the default UI camera later, once one exists. Build new
interface from `bevy_ui`, not hand-placed sprites.

UI is rendered by the **upscale camera**, not the world camera. That is forced:
`bevy_ui`'s focus system only computes a cursor position for cameras whose render target
is a window, and the world camera renders to an off-screen image, so `Interaction` would
never fire there. `UiScale` is `PIXEL_SCALE`, so all `Val::Px` and font sizes are in
canvas pixels (`ui::FONT_BODY` is 6, not 24).

### One highlight, three devices (`src/ui/nav.rs`)

Screens do **not** keep a selection index. A widget spawns with `Focusable::new(row, col)`
and the mouse, the keyboard and a gamepad all move the same `Focus`; choosing anything —
click, `enter`, gamepad `A` — arrives as one `Activated(Entity)` message, and `esc`/`B`
as `Cancelled`. Directional movement is worked out from the row/col coordinates
(`nav::step`, unit-tested), so a list of rows with buttons along them navigates the way it
looks. Four rules are load-bearing and were each a bug first:

- **`Scope` is what makes a modal modal.** Focus never leaves the current scope, so an
  open dialog or on-screen keyboard cannot be navigated past or clicked through.
- **`KeyboardCapture`** turns the physical keyboard off as a navigation device while a
  text field owns it — otherwise typing `wasd` walks the highlight, and `enter` both ends
  the name and presses whatever the highlight was on. It is a frame-start snapshot, not a
  live read of the field, or the keypress that *opens* a field is also typed into it.
- **Input belongs to the screen it was made on.** Messages outlive their frame, so
  `nav::drop_input_from_the_last_screen` clears them on a state transition; without it one
  `esc` in the editor falls through the browser and quits the game. Screens run
  `.after(NavSystems)`.
- **Modals need `GlobalZIndex`.** A later-spawned UI root is not reliably on top.

A gamepad cannot type, so `ui/keyboard.rs` is an on-screen keyboard whose keys are
ordinary focusables writing into one `TextEntry` buffer that the physical keyboard also
writes to.

## The map browser (`src/browser.rs`)

The screen that owns saved maps: create with a name, play, edit, duplicate, delete (with
a confirmation, since it is the only button here that destroys work), and a scroll view
for the list. A map's **name** is the button that plays it and `edit` is the one beside
it — playing is what a map is for, so it gets the biggest target in the row. Scrolling
follows the focus (`ui::scroll_to_show`) so a controller can reach the end of a long
list, and the wheel moves it directly for a mouse. `CROWD2X_MAPS` points the store
somewhere else, which is what keeps a QA run from deleting real maps.
