#!/bin/sh
set -eu

api_base_url="${VITE_API_BASE_URL:-http://localhost:5000}"
web_root="${WEB_ROOT:-/usr/share/nginx/html}"
escaped_api_base_url="$(printf '%s' "$api_base_url" | sed "s/\\\\/\\\\\\\\/g; s/'/\\\\'/g")"

cat > "$web_root/env.js" <<EOF
window.__MTY_CONFIG__ = {
  VITE_API_BASE_URL: '${escaped_api_base_url}'
};
EOF

exec "$@"
