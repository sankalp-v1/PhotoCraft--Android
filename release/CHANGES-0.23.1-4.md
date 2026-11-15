# 📝 PhotoCraft for Android 0.23.1-4 (Smooth Pen Edition): what changed

Based on [morte2437/photocraft-android](https://github.com/morte2437/photocraft-android),
branch `android-port`, commit `e4258e9` (0.23.1-1). All the Android porting work is
**@morte2437**'s; PhotoCraft is by the ArtCraft team and the PhotoCraft contributors. This file
lists only what this fork changed.

APK: `photocraft-0.23.1-4-arm64-v8a.apk`, versionCode 230104, Android 8.0+ (API 26), arm64-v8a.
SHA-256 `d4389652b48c80a59db91c0e293ead0367e1d97f007ea3228d7ad411824bdbec`.

## ✒️ Pen lag (original issue #11)

Where the time went (the Perfetto numbers in #11, Galaxy Tab S10 FE, 90 Hz): the pen's sample
reached the app in 1.5 ms, but a frame took 34–35 ms from its start to the screen, 23 ms of it
waiting in the display's queue ("buffer stuffing"). Every pen sample (240 a second) asked for a
frame, egui draws one more frame after any frame with input, and with vsync each frame joins a
queue behind the display, so the queue was always full.

1. **Frames no longer queue** (`apps/photocraft-android/src/app.rs`, `src/pace.rs`): the swap
   chain shows the newest frame at each refresh (Vulkan mailbox; swap interval 0 on GLES; no
   tearing on Android). The app paces itself: twice per refresh (120–240 frames/s) while pen,
   finger or mouse are at work, the display's rate for animations, nothing when idle.
   New setting: Window › Touch Input › Touch Input Settings › Pen › **Low latency drawing** (on
   by default, applies at the next start). Off restores the old behaviour exactly.
2. **The pen's own samples are the stroke** (`src/pen_path.rs`, `crates/tablet/src/android.rs`):
   samples are drawn as soon as the Activity's pen hook has them; winit's later copies are
   dropped so a stroke never steps backwards. A stroke whose samples disagree with winit's falls
   back to winit's moves; samples of a stroke the app never saw can't leak into the next one.
3. **The pen thread never blocks:** a small waker thread asks egui for frames, instead of
   Android's UI thread waiting on egui's lock while a frame tessellates.
4. **Faster frames** (`PhotoCraftActivity.java`): the drawing thread gets display priority,
   Android 12+ performance hints (ADPF) keep its CPU core fast during strokes, and the display is
   asked for its highest refresh rate at the current resolution.

Timing model (90 Hz, 2–8 ms frames): newest pen sample on screen ~36 ms old → ~8–14 ms old.

## 🧩 Layer Style on tablets and phones (original issue #36)

- Reproduced in a UI test: on a 360-point phone the dialog was 676 points wide (effect list +
  page + button column side by side) and ran off both edges of the screen. In the tablet layout
  in split screen (640 points) it was 664 points wide.
- Narrower than its three columns, it now stacks: OK / Cancel / New Style… at the top, then
  **Effects | page** tabs. Picking an effect opens its page. Tablets at full screen keep the
  desktop layout unchanged.
- It never grows bigger than the window. On an upright phone it takes 55% of the height (the
  image stays in view above it), sized so nothing is cut off.
- New tests `phone_layer_style_fits_the_window` and `tablet_layer_style_fits_the_window`:
  14 window sizes.

## 🛡️ Robustness

- `AndroidManifest.xml`: `configChanges` covers every change Android would restart the Activity
  for (font size, language, bold text, SIM, touchscreen, colour mode…). A restart can't start
  the Rust app twice in one process, so it used to end the session.
  `appCategory="productivity"` so device makers' game services don't throttle it as a game.
- The ink layer, the pointer icon and the performance hints can't crash the input path.
- An unreadable picker request is dropped, and answered when its id is readable, instead of
  being retried every 150 ms for ever (which blocked every later Open / Save As).
- The display listener is unregistered when the Activity goes.
- A pen that touches down beside a resting palm starts a new stroke path.
- The in-app updater follows `sankalp-v1/PhotoCraft--Android` releases (tag
  `android-v<version>-<build>` with the `.apk` attached), whose APKs share this build's signing key.
- `build.sh` runs the Gradle wrapper through `sh`; upstream's CI workflows and Dependabot are
  parked in `docs/upstream-github/` so they don't run in this repository.

## 🧪 Verified, and not

- 131 / 131 host unit tests (`cargo test -p photocraft-android -p photocraft-tablet`), 17 new:
  pen-path races (200 random interleavings of the two input streams), lifts, cancels,
  mismatched coordinates, floods, and a display model of the old and new presentation.
- Full `photocraft-ui-egui` suite: 7,457 pass. The ones that fail also fail on the untouched
  upstream code on the build machine (missing CJK and Arabic fonts and translation catalogs,
  low free disk). None of them involve the changed code.
- `cargo clippy` for `aarch64-linux-android`: no warnings in the changed crates.
- APK: v2 signature, 16 KB aligned, all 15 native hooks exported, Gradle lint-vital passes.
- ⚠️ Not yet run on a physical device or emulator. The latency gain comes from the model and the
  measurements reported in #11.

## 📲 Installing

Signed with a different key than morte2437's official releases, so Android won't install it over
them: export your settings (Edit › Preferences › Export Settings…), uninstall, install this APK,
import the settings.
