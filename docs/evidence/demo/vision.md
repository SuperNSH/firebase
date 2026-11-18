# Demo vision record (2026-11-19)

Every PNG below was opened and looked at.

- `native-ios/host-launch.png`: iPhone simulator (nsh-firebase-ios, iOS 27). The host header says
  configured / bundle loaded / apns token set; the demo's home list (Authentication, Realtime
  Database, Cloud Storage, Cloud Messaging, Analytics, Performance) renders below it.
- `native-ios/monitor-phone.png`: Performance screen on the phone. `/slow` at 1209 ms is the top
  row, above `/assert`, `/collect`, `/health` at 6-7 ms, all 200, 0 errors. The host header shows the
  two native `firebase.perf.startTrace`/`stopTrace` pairs the screen runs.
- `native-android/monitor-phone.png`: same screen on the Android emulator, through `adb reverse`
  to the Mac's collector. 1207 ms `/slow` is on top; the four failed rows below it are from the
  first load, taken before the reverse port existed (127.0.0.1 inside the emulator is the emulator).
- Earlier macOS captures (`macos-*.png`, `perfmon/monitor-*.png`) are unchanged.
