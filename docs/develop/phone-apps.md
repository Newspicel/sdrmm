---
title: Phone apps
description: Build and test the optional iOS and Android companion apps on sdrmm-mobile-core.
---

Shared core (`crates/mobile-core`, UniFFI): `cargo xtask mobile bindings | check | ios | android`.

## iPhone

`apps/ios` (Xcode 27, iPhone 17 / iOS 27 simulator; override with `SDRMM_IOS_SIMULATOR`):

- `cargo xtask ios generate | build | test [--ui] | lint | e2e | archive | upload`
- Physical device: set `DEVELOPMENT_TEAM = <team>` in `apps/ios/Config/Local.xcconfig`.

## Android

`apps/android` (Kotlin, Jetpack Compose, Gradle 9.8, API 29+, JDK 21; Gradle calls `cargo xtask mobile android` directly, pass `-Psdrmm.abis=arm64-v8a` for one ABI):

```sh
sdkmanager "platform-tools" "emulator" \
  "platforms;android-37.0" "build-tools;37.0.0" "ndk;30.0.16248370" \
  "system-images;android-36;google_apis_playstore;arm64-v8a" "extras;google;auto"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/30.0.16248370"
```

```sh
cd apps/android
./gradlew spotlessCheck checkSourceRules lintDebug testDebugUnitTest assembleDebug
"$ANDROID_HOME/build-tools/37.0.0/zipalign" -c -P 16 -v 4 app/build/outputs/apk/debug/app-debug.apk
```

<details>
<summary>Emulator tests, release signing, and Android Auto head unit</summary>

- **Emulator (`10.0.2.2` reaches host; spoof GPS `adb emu geo fix 13.4050 52.5200`):** `./gradlew connectedDebugAndroidTest`
- **Release signing:** `SDRMM_KEYSTORE`, `SDRMM_KEYSTORE_PASSWORD`, `SDRMM_KEY_ALIAS`, `SDRMM_KEY_PASSWORD` for `assembleRelease` / `bundleRelease`.
- **Android Auto DHU:** enable **Developer settings → Unknown sources → Start head unit server**, then `adb forward tcp:5277 tcp:5277 && "$ANDROID_HOME/extras/google/auto/desktop-head-unit"`.

</details>
