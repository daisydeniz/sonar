# app/

macOS app (SwiftUI + AppKit). Created in task T0.4 with XcodeGen
(`project.yml`). The `.xcodeproj` is generated — do not commit it.

The Rust core is linked in through `app/Generated/` (gitignored), produced by
`scripts/build-core.sh` (also run by `scripts/check.sh`): it builds
`sonar-ffi` as an arm64 static lib, packs it into `SonarCore.xcframework`
(C module `SonarCoreFFI`) and writes the UniFFI Swift bindings to
`SonarCore.swift`, which the app compiles as source. `SONAR_CORE_PROFILE=release`
builds an optimized core (default `debug`). There is no Xcode script phase for
cargo: after changing Rust code, re-run the script, then build in Xcode.

Fresh clone / opening in Xcode:

```sh
scripts/build-core.sh               # app/Generated/ (must exist before xcodegen)
(cd app && xcodegen generate)       # app/Sonar.xcodeproj
open app/Sonar.xcodeproj
```

The app is arm64-only for now (`ARCHS: arm64` in `project.yml`) because the
core xcframework has no x86_64 slice yet. Core calls from Swift go through
`Sonar/Services/` (`CoreInfo` today; `CoreBridge` later) — only trivial calls
may run on the main actor, real core work runs on background tasks.
