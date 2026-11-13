validate_domain() {
    local label='[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?'
    local domain_pattern="^($label\.)+$label$"
    if [[ ${#1} -gt 253 || ! $1 =~ $domain_pattern || ! ${1##*.} =~ [a-z] ]]; then
        printf 'Expected a DNS hostname without a scheme, port or path: %s\n' "$1" >&2
        return 1
    fi
}

install_site_config() {
    local domain=$1 template=$2 destination=$3 rendered
    rendered=$(sed "s/@SYNC_DOMAIN@/$domain/g" "$template") || return
    printf '%s\n' "$rendered" | install -m 0644 /dev/stdin "$destination"
}
