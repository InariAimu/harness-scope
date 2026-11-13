#!/bin/bash
set -euo pipefail
if [[ $# -lt 1 || $# -gt 2 ]]; then
    printf 'Usage: bash %s DOMAIN [EXPECTED_IPV4]\n' "$0" >&2
    exit 1
fi
files=$(dirname -- "${BASH_SOURCE[0]}")
source "$files/site-config.sh"
domain=${1,,}
validate_domain "$domain"
test "$(id -u)" = 0 || { echo 'Run as root' >&2; exit 1; }
if [[ $# -eq 2 ]]; then
    expected_ipv4=$2
    if ! python3 -c 'import ipaddress, sys; ipaddress.IPv4Address(sys.argv[1])' "$expected_ipv4" 2>/dev/null; then
        printf 'Invalid expected IPv4 address: %s\n' "$expected_ipv4" >&2
        exit 1
    fi
    if ! getent ahostsv4 "$domain" | awk '{print $1}' | grep -Fx -- "$expected_ipv4" >/dev/null; then
        printf 'Wait for %s A record to resolve to %s\n' "$domain" "$expected_ipv4" >&2
        exit 1
    fi
fi
certbot certonly --webroot -w /var/www/harness-scope-acme -d "$domain" --cert-name "$domain" \
    --non-interactive --agree-tos \
    --deploy-hook '/usr/sbin/nginx -t && /usr/bin/systemctl reload nginx'
install_site_config "$domain" "$files/nginx.conf" "/etc/nginx/sites-available/$domain"
nginx -t
systemctl reload nginx
curl --fail --silent --show-error --retry 10 --retry-all-errors --retry-delay 1 "https://$domain/health"
printf '\nHTTPS enabled\n'
