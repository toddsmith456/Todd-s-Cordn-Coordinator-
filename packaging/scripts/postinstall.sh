#!/bin/sh
set -e

if ! id cordn >/dev/null 2>&1; then
	if command -v useradd >/dev/null 2>&1; then
		useradd --system --no-create-home --home-dir /var/lib/cordn --shell /usr/sbin/nologin --user-group cordn || true
	elif command -v adduser >/dev/null 2>&1; then
		addgroup -S cordn 2>/dev/null || true
		adduser -S -D -H -h /var/lib/cordn -s /sbin/nologin -G cordn cordn 2>/dev/null || true
	fi
fi

mkdir -p /var/lib/cordn /etc/cordn
if id cordn >/dev/null 2>&1; then
	chown cordn:cordn /var/lib/cordn || true
	chmod 0750 /var/lib/cordn || true
	if [ -f /etc/cordn/cordn.env ]; then
		chown root:cordn /etc/cordn/cordn.env || true
		chmod 0640 /etc/cordn/cordn.env || true
	fi
fi

if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
	systemctl daemon-reload >/dev/null 2>&1 || true
fi
