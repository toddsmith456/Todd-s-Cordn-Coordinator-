# Todd’s Cordn Coordinator fork

Source: https://github.com/Cordn-msg/cordn-rs
Base commit: 9aff92835f721c0de66faeaff1d0b56b4ee67767
Destination: https://github.com/toddsmith456/Todd-s-Cordn-Coordinator-

The only app change is the installation identity:
`org.cordn.coordinator` → `com.toddsmith456.cordn.coordinator`.
The Android namespace/MainActivity package follows the new identifier for
Tauri-generated activity compatibility. Shared Kotlin service/JNI package
names stay unchanged; they are internal classes, not installation IDs.
The FileProvider authority already derives from `${applicationId}`.
App label, icons, UI, protocol, and coordinator implementation are unchanged.

Release infrastructure uses this fork's own signing key and checks the
resulting APK's package ID and signer fingerprint. See the Android README.
The upstream Git history and vendor/contextvm-sdk branch are preserved.
Historical upstream tags represent upstream releases, not signed fork APKs.
No private signing material belongs in this repository.
