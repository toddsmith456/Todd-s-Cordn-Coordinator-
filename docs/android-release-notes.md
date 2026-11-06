## Scheduled wakeup restarts

- New **Scheduled restarts** panel fits the existing dark green UI.
- Enable once or twice daily and choose local restart times (15+ minutes apart).
- Android exact wakeup alarms can run while the phone sleeps; the UI guides you to Alarms & reminders permission.
- Shows the next wakeup; restores alarms after reboot, app upgrade, or clock/time-zone changes.
- **Manual Stop is respected.** Alarms never start a stopped coordinator.
- Graceful restart retains the coordinator identity, settings, and stored data; the native runtime exits before restarting.
- Off by default. Android force-stop disables alarms until you reopen the app; manufacturer battery restrictions may affect delivery.

### Install or update

Install **arm64** for most modern Android phones, **armv7** for compatible 32-bit devices, or **x86_64** for compatible emulators/devices. This update uses the same fork package ID and signing key, so install it over your existing Todd fork—no uninstall needed.

The original Cordn app remains separately installed.

Signing-certificate SHA-256: `80f84f7d629664ebeca758e31b8a1e712c10d89f6efebec4dca2bb37ef0997d2`.
All APKs are signature/package checked by CI; `SHA256SUMS` contains download checksums.
