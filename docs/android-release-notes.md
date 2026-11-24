## Private beta: publish relay list fix

- Fixes "Publish Relay List" failing with `invalid args \`payload\` for command \`publish_relay_list\`: missing field \`privateKeyHex\``. The command no longer requires the frontend to send private key material; identity/config are resolved on the native side (Android KeyStore / local identity file).
- Removes a duplicate desktop `publish_relay_list` method that returned `Unsupported`.

**Beta build** — intended for testing the relay-list publish flow. Install over the existing Todd fork; do not uninstall or clear app data unless you intend to reset.

Signing-certificate SHA-256: `80f84f7d629664ebeca758e31b8a1e712c10d89f6efebec4dca2bb37ef0997d2`.
