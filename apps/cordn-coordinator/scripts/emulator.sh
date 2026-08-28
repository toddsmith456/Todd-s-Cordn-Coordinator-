#!/usr/bin/env bash
# Boot the Android emulator and run the coordinator app on it (tauri android dev).
#
#   pnpm emulator                  boot + dev on the default AVD
#   CORDN_AVD=Pixel_7 pnpm emulator    use another AVD
#   EMU_HEADLESS=1 pnpm emulator    boot without a window (CI-style)
set -euo pipefail

avd="${CORDN_AVD:-Medium_Phone}"
sdk="${ANDROID_HOME:-$HOME/Android/Sdk}"
adb="$sdk/platform-tools/adb"
emulator_bin="$sdk/emulator/emulator"

[ -x "$adb" ] || { echo "adb not found at $adb (set ANDROID_HOME)" >&2; exit 1; }
[ -x "$emulator_bin" ] || { echo "emulator not found at $emulator_bin (set ANDROID_HOME)" >&2; exit 1; }

if [ -n "$("$adb" devices | awk 'NR>1 && $2=="device"{print $1}')" ]; then
  echo "-> device/emulator already connected, reusing it"
else
  echo "-> booting the '$avd' emulator (log: /tmp/cordn-emulator.log)"
  flags=(-avd "$avd" -no-audio)
  if [ "${EMU_HEADLESS:-0}" = 1 ]; then
    flags+=(-no-window -gpu swiftshader_indirect)
  fi
  nohup "$emulator_bin" "${flags[@]}" >/tmp/cordn-emulator.log 2>&1 &
  booted=0
  for _ in $(seq 1 90); do
    if [ "$("$adb" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; then
      booted=1
      break
    fi
    sleep 2
  done
  if [ "$booted" != 1 ]; then
    echo "emulator did not boot within 3 minutes — see /tmp/cordn-emulator.log" >&2
    exit 1
  fi
  echo "-> emulator booted"
fi

cd "$(dirname "$0")/.."
exec pnpm exec tauri android dev
