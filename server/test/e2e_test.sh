#!/usr/bin/env bash
set -euo pipefail

echo "=================================================="
echo "Starting Mieru Admin & Mita End-to-End Container Test"
echo "=================================================="

# 1. Provide systemctl container stub (only in isolated Docker test containers without real systemd daemon)
if ! command -v systemctl >/dev/null 2>&1; then
  echo "[CONTAINER NOTE] Providing systemctl stub for isolated test container environment"
  printf '#!/bin/sh\nexit 0\n' > /usr/bin/systemctl
  chmod +x /usr/bin/systemctl
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIST_DIR="${SCRIPT_DIR}/../dist"
if [ ! -f "${DIST_DIR}/mita_3.38.0_amd64.deb" ] && [ -f "/dist/mita_3.38.0_amd64.deb" ]; then
  DIST_DIR="/dist"
fi

# 2. Install official mita deb package
echo "[1/7] Installing mita 3.38.0 deb package..."
dpkg -i "${DIST_DIR}/mita_3.38.0_amd64.deb"
mita version

MITA_PID=""
SBOX_PID=""
ADMIN_PID=""

# Cleanup on exit
trap 'kill $ADMIN_PID 2>/dev/null || true; kill $MITA_PID 2>/dev/null || true; kill $SBOX_PID 2>/dev/null || true' EXIT

# 3. Setup directories and token for mieru-admin
echo "[2/7] Configuring mieru-admin token (0600)..."
mkdir -p /etc/mieru-admin /etc/mita /var/run/mita
chmod 775 /var/run/mita
chown mita:mita /var/run/mita 2>/dev/null || true

TOKEN="test-e2e-token-1234567890abcdef"
echo "$TOKEN" > /etc/mieru-admin/token
chmod 600 /etc/mieru-admin/token

# 4. Start mieru-admin daemon in background
echo "[3/7] Starting mieru-admin management daemon..."
"${DIST_DIR}/mieru-admin" serve --listen 127.0.0.1:9081 --token-file /etc/mieru-admin/token --config-file /etc/mita/server_config.json &
ADMIN_PID=$!
sleep 2

# Verify API status
echo "[4/7] Testing Mieru Admin API..."
STATUS_CODE=$(curl -s -o /tmp/status.json -w "%{http_code}" -H "Authorization: Bearer $TOKEN" http://127.0.0.1:9081/v1/status)
if [ "$STATUS_CODE" != "200" ]; then
  echo "FAIL: API status returned $STATUS_CODE"
  cat /tmp/status.json
  exit 1
fi
echo "✓ API /v1/status returned HTTP 200"

# Add User via API
echo "Adding user via API..."
USER_CODE=$(curl -s -o /tmp/adduser.json -w "%{http_code}" -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name": "e2e_test@example.com", "password": "DummySecurePassword999!"}' \
  http://127.0.0.1:9081/v1/users)

if [ "$USER_CODE" != "201" ]; then
  echo "FAIL: Add user returned $USER_CODE"
  cat /tmp/adduser.json
  exit 1
fi
echo "✓ API /v1/users returned HTTP 201 Created"

# Export Profile with valid password into /tmp/profile.json
echo "Exporting client profile with correct password..."
EXPORT_CODE=$(curl -s -o /tmp/profile.json -w "%{http_code}" -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"username": "e2e_test@example.com", "password": "DummySecurePassword999!", "server_host": "127.0.0.1", "server_port": 28443}' \
  http://127.0.0.1:9081/v1/profiles/export)

if [ "$EXPORT_CODE" != "200" ]; then
  echo "FAIL: Export profile returned $EXPORT_CODE"
  cat /tmp/profile.json
  exit 1
fi
echo "✓ API /v1/profiles/export returned HTTP 200"

# Export Profile with wrong password must fail with 401
echo "Testing export with invalid password..."
WRONG_EXPORT_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"username": "e2e_test@example.com", "password": "WrongPassword!", "server_host": "127.0.0.1"}' \
  http://127.0.0.1:9081/v1/profiles/export)

if [ "$WRONG_EXPORT_CODE" != "401" ]; then
  echo "FAIL: Export with wrong password expected 401, got $WRONG_EXPORT_CODE"
  exit 1
fi
echo "✓ API /v1/profiles/export rejected invalid password with HTTP 401"

# 5. Start mita proxy daemon in foreground mode
echo "[5/7] Starting mita server proxy..."
export MITA_INSECURE_UDS=1
export MITA_CONFIG_JSON_FILE=/etc/mita/server_config.json
/usr/bin/mita run &
MITA_PID=$!
sleep 3

# Wait for mita port 28443 to be open
PORT_OPEN=0
for i in $(seq 1 10); do
  if (echo > /dev/tcp/127.0.0.1/28443) 2>/dev/null; then
    echo "✓ Mita TCP proxy listening on 127.0.0.1:28443"
    PORT_OPEN=1
    break
  fi
  sleep 1
done

if [ "$PORT_OPEN" -eq 0 ]; then
  echo "FAIL: Mita proxy did not bind to port 28443 in time"
  exit 1
fi

# 6. Configure and start sing-box client using the EXACT exported JSON profile
echo "[6/7] Generating sing-box outbound from real exported profile.json..."
python3 -c '
import json
with open("/tmp/profile.json") as f:
    prof = json.load(f)

cfg = {
    "log": {"level": "info"},
    "inbounds": [{
        "type": "mixed",
        "tag": "mixed-in",
        "listen": "127.0.0.1",
        "listen_port": 1080
    }],
    "outbounds": [{
        "type": "mieru",
        "tag": "mieru-out",
        "server": prof["server"],
        "server_port": int(prof["server_port"]),
        "transport": prof["transport"],
        "username": prof["username"],
        "password": prof["password"]
    }],
    "route": {
        "final": "mieru-out",
        "auto_detect_interface": True
    }
}
with open("/tmp/sing-box.json", "w") as out:
    json.dump(cfg, out, indent=2)
'

/usr/bin/sing-box run -c /tmp/sing-box.json &
SBOX_PID=$!
sleep 3

# 7. Test proxy request through sing-box -> mita -> internet
echo "[7/7] Testing end-to-end traffic proxying through mbox (sing-box) and mita..."
CURL_HTTP_CODE=$(curl -s -x socks5h://127.0.0.1:1080 --max-time 15 -o /tmp/resp.html -w "%{http_code}" http://example.com || true)

echo "Response HTTP code: $CURL_HTTP_CODE"
if [ "$CURL_HTTP_CODE" != "200" ]; then
  echo "FAIL: Proxy HTTP request failed (returned code $CURL_HTTP_CODE, expected 200)"
  exit 1
fi
echo "✓ End-to-end HTTP request succeeded through Mieru proxy (HTTP 200 OK)!"

# 8. Test refusal with wrong password in sing-box client
echo "Testing sing-box rejection with wrong password..."
cat << 'EOF' > /tmp/sing-box-wrong.json
{
  "log": {"level": "info"},
  "inbounds": [{"type": "mixed", "tag": "mixed-in", "listen": "127.0.0.1", "listen_port": 1081}],
  "outbounds": [{
    "type": "mieru",
    "tag": "mieru-out",
    "server": "127.0.0.1",
    "server_port": 28443,
    "transport": "TCP",
    "username": "e2e_test@example.com",
    "password": "BadPassword123"
  }],
  "route": {"final": "mieru-out"}
}
EOF
/usr/bin/sing-box run -c /tmp/sing-box-wrong.json &
WRONG_SBOX_PID=$!
sleep 2

WRONG_REQ_CODE=$(curl -s -x socks5h://127.0.0.1:1081 --max-time 4 -o /dev/null -w "%{http_code}" http://example.com || true)
kill $WRONG_SBOX_PID 2>/dev/null || true

if [ "$WRONG_REQ_CODE" = "200" ]; then
  echo "FAIL: invalid password request unexpectedly succeeded with 200"
  exit 1
fi
echo "✓ Mita successfully refused connection with invalid password (HTTP code: $WRONG_REQ_CODE)"

echo "=================================================="
echo "ALL END-TO-END CONTAINER TESTS PASSED STRICTLY!"
echo "=================================================="
