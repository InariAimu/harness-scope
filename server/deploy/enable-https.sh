#!/bin/bash
set -euo pipefail
test "$(id -u)" = 0 || { echo 'Run as root' >&2; exit 1; }
getent ahostsv4 hs.amu.moe | awk '{print $1}' | grep -Fx 49.51.191.33 >/dev/null || {
    echo 'Wait for hs.amu.moe A record to resolve to 49.51.191.33' >&2
    exit 1
}
certbot certonly --webroot -w /var/www/harness-scope-acme -d hs.amu.moe \
    --non-interactive --agree-tos \
    --deploy-hook '/usr/sbin/nginx -t && /usr/bin/systemctl reload nginx'
install -m 0644 /opt/harness-scope/src/server/deploy/nginx.conf /etc/nginx/sites-available/hs.amu.moe
nginx -t
systemctl reload nginx
curl --fail --silent --show-error --retry 10 --retry-all-errors --retry-delay 1 https://hs.amu.moe/health
printf '\nHTTPS enabled\n'
