#!/bin/bash
set -euo pipefail
test "$(id -u)" = 0 || { echo 'Run as root' >&2; exit 1; }
base=/opt/harness-scope
files="$base/src/server/deploy"
binary="$base/build-target/release/harness-scope-service"
test -x "$binary"
if ! id harness-scope >/dev/null 2>&1; then
    useradd --system --home-dir /var/lib/harness-scope --shell /usr/sbin/nologin harness-scope
fi
install -d -m 0755 "$base/releases" /etc/harness-scope /var/www/harness-scope-acme
install -d -o harness-scope -g harness-scope -m 0700 /var/lib/harness-scope /var/backups/harness-scope
if [ ! -f /etc/harness-scope/server.env ]; then
    (umask 077; python3 -c 'import secrets; print("HARNESS_SCOPE_TOKEN=" + secrets.token_urlsafe(48))' > /etc/harness-scope/server.env)
fi
chmod 0600 /etc/harness-scope/server.env
release="$base/releases/$(date -u +%Y%m%dT%H%M%SZ)"
install -d -m 0755 "$release"
install -m 0755 "$binary" "$release/harness-scope-service"
install -m 0644 "$files/backup.py" "$release/backup.py"
if [ -e "$base/current" ] && [ ! -L "$base/current" ]; then
    echo 'Refusing to replace a non-symlink current path' >&2
    exit 1
fi
ln -sfn "$release" "$base/current"
install -m 0644 "$files/harness-scope.service" /etc/systemd/system/harness-scope.service
install -m 0644 "$files/harness-scope-backup.service" /etc/systemd/system/harness-scope-backup.service
install -m 0644 "$files/harness-scope-backup.timer" /etc/systemd/system/harness-scope-backup.timer
install -m 0644 "$files/nginx-proxy.conf" /etc/nginx/snippets/harness-scope-proxy.conf
if [ ! -e /etc/nginx/sites-available/hs.amu.moe ]; then
    install -m 0644 "$files/nginx-http.conf" /etc/nginx/sites-available/hs.amu.moe
fi
if [ ! -e /etc/nginx/sites-enabled/hs.amu.moe ]; then
    ln -s /etc/nginx/sites-available/hs.amu.moe /etc/nginx/sites-enabled/hs.amu.moe
fi
nginx -t
systemctl daemon-reload
systemctl enable harness-scope.service harness-scope-backup.timer
systemctl restart harness-scope.service
systemctl start harness-scope-backup.timer
systemctl reload nginx
curl --fail --silent --show-error --retry 10 --retry-connrefused --retry-delay 1 http://127.0.0.1:8787/health
printf '\nInstalled release %s\n' "$release"
