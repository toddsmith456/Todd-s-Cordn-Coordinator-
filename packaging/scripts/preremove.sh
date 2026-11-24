#!/bin/sh
set -e

if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
	systemctl --no-reload disable --now cordn.service >/dev/null 2>&1 || true
elif command -v rc-service >/dev/null 2>&1; then
	rc-service cordn stop >/dev/null 2>&1 || true
	rc-update del cordn default >/dev/null 2>&1 || true
fi
