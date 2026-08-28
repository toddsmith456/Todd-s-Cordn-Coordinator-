# Cordn Coordinator for Android

A SvelteKit/Tauri shell that runs the existing Rust cordn server in an Android
foreground service.

The UI and coordinator intentionally use separate Android processes. Closing or
restarting the WebView does not interrupt the coordinator:

```text
SvelteKit UI → Tauri plugin → CordnService (:coordinator) → JNI → cordn-server
```

The service stores its SQLite database under Android's no-backup app directory.
Its Nostr private key is encrypted by a non-exportable Android Keystore key and
is never written to the web frontend or configuration file. The collapsible
Activity & logs panel polls live record counts and the latest 64 KiB of the
app-private service log only while it is open. The Connect & share panel shows
a QR code and `cordn.net` link for the coordinator (npub / nprofile / hex
display switch), and the configuration's danger zone can erase coordinator
data or reset the identity (both require the service to be stopped).

The identity store is a plain file (`no_backup/cordn-identity.json`), not
SharedPreferences: the file is read from the `:coordinator` process and reset
from the app process, and SharedPreferences cache per process.

## Development

Requirements: pnpm, Rust, Android SDK 36, an Android NDK, and the Rust Android
targets required by the device/emulator.

```bash
pnpm install
pnpm check
pnpm build

# Generate the Android project once (already committed here)
pnpm exec tauri android init

# Run interactively on a device/emulator
pnpm android

# Build an APK
pnpm android:build
```

For a faster local build, select one ABI:

```bash
pnpm exec tauri android build --debug --apk --target aarch64 --ci
pnpm exec tauri android build --debug --apk --target x86_64 --ci
```

The generated APK is under
`src-tauri/gen/android/app/build/outputs/apk/`. Regenerate branded and adaptive
icons with `pnpm exec tauri icon app-icon.json`; the padded white mark is derived
from `cordn-web/static/cordn-logo.svg`. Android-specific service code lives in `src-tauri/plugins/cordn-android-service`; do not duplicate coordinator
logic there—shared server assembly belongs in `crates/cordn-server/src/runtime.rs`.

## GitHub release APK

The tag-triggered release workflow builds, signs, verifies, and attaches an
arm64 APK to the GitHub Release. It intentionally reuses the Cordn release
identity already used by `cordn-web`. Give this repository access to the same
organization secrets, or copy the existing values into repository secrets:

- `CORDN_KEYSTORE_BASE64`
- `CORDN_KEYSTORE_PASSWORD`
- `CORDN_KEY_ALIAS`
- `CORDN_KEY_PASSWORD`

GitHub does not expose existing secret values, so repository-scoped secrets in
`cordn-web` cannot be read or copied by this workflow; use the offline keystore
backup or make the secrets organization-scoped for both repositories. CI rejects
any APK whose signing-certificate SHA-256 is not Cordn's published fingerprint:
`a6626cfca551c5605f0af450c6bf6d831fc70eb2dcd17f027eb358c56852758f`.
Losing or changing that keystore prevents future APKs from updating existing
installs.
