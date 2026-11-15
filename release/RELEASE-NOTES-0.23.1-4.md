## 🎨 PhotoCraft for Android 0.23.1-4: Smooth Pen Edition ✒️⚡

> 🙏 Built on **[@morte2437](https://github.com/morte2437)**'s Android port ([morte2437/photocraft-android](https://github.com/morte2437/photocraft-android)) of **PhotoCraft** by the ArtCraft team & contributors ([storytold/photocraft](https://github.com/storytold/photocraft)). All the porting work is theirs ❤️. This build adds the fixes below.

### ✨ What's new
- ⚡ **Much lower pen lag:** frames no longer queue behind the display, pen samples are drawn the moment they arrive, and the drawing thread gets priority and a fast CPU (fixes the ~35 ms delay reported in morte2437/photocraft-android#11).
- 🎚️ **New setting:** Touch Input Settings › Pen › *Low latency drawing* (on by default; turn off for the old behaviour).
- 🧩 **Layer Style fits every screen:** full desktop layout on tablets, a clean stacked layout with tabs in split screen, DeX windows and on phones (fixes morte2437/photocraft-android#36).
- 🛡️ **No more lost sessions** when you change font size, language or bold text, or plug in a keyboard or mouse. Crash-proofed pen input.
- 🔄 The in-app updater now follows this repository.

### 📲 Install
- 📥 Download **`photocraft-0.23.1-4-arm64-v8a.apk`** below. Needs Android 8.0+, arm64.
- ⚠️ Coming from morte2437's official APK? Export your settings, **uninstall** it, install this one, then import your settings (it's signed with a different key).
- ⚠️ Not yet tested on a physical tablet. If drawing feels off, switch off *Low latency drawing* and open an issue 🙏

🔐 SHA-256: `d4389652b48c80a59db91c0e293ead0367e1d97f007ea3228d7ad411824bdbec`
📝 Full changelog: [`release/CHANGES-0.23.1-4.md`](https://github.com/sankalp-v1/PhotoCraft--Android/blob/main/release/CHANGES-0.23.1-4.md)
