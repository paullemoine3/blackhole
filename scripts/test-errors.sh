#!/usr/bin/env bash
# Error pages on macOS, driven by the devtools harness. Each failure must show our page (never a
# blank tab, never a crash): refused connection, retry, unknown host (DNS), restricted port,
# restricted port typed in an open tab, connection dropped by the server, HTTP 500 / 403 without a body, self-signed certificate, an
# invalid address typed in the bar; then a working page still loads. No network needed.
# Local servers on 127.0.0.1:47996-47998 (47999 stays closed). Scratch profile /tmp/bh_data
# (wiped first); screenshots in /tmp/bh_shots/err-*.png.
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
work=$(mktemp -d)
pids=()
cleanup() { kill "${pids[@]}" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT

# 47998: accepts, then closes the connection without a word.
python3 -I -c '
import socket
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", 47998)); s.listen(8)
while True:
    c, _ = s.accept(); c.close()
' & pids+=($!)
# 47997: /500 and /403 with an empty body, anything else a small page titled "bh ok".
cat > "$work/http.py" <<'P'
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        code = {"/500": 500, "/403": 403}.get(self.path, 200)
        body = b"" if code != 200 else b"<!doctype html><title>bh ok</title><p>ok</p>"
        self.send_response(code)
        self.send_header("Content-Type", "text/html")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a):
        pass
port = int(sys.argv[1])
srv = HTTPServer(("127.0.0.1", port), H)
if len(sys.argv) > 2:
    import ssl
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(sys.argv[2], sys.argv[3])
    srv.socket = ctx.wrap_socket(srv.socket, server_side=True)
srv.serve_forever()
P
python3 -I "$work/http.py" 47997 & pids+=($!)
# 47996: the same over https with a self-signed certificate.
openssl req -x509 -newkey rsa:2048 -nodes -days 2 -subj "/CN=127.0.0.1" \
  -keyout "$work/key.pem" -out "$work/cert.pem" >/dev/null 2>&1
python3 -I "$work/http.py" 47996 "$work/cert.pem" "$work/key.pem" & pids+=($!)
sleep 1

H="eval content document.querySelector('h1') ? document.querySelector('h1').textContent : 'none'"
U="eval content document.getElementById('url') ? document.getElementById('url').textContent : 'none'"
cat > /tmp/bh_script.txt <<S
wait 2000
cmd {"cmd":"open","input":"http://127.0.0.1:47999/"}
wait 5000
$H
$U
eval content typeof window.__bhRetry
shot err-refused
eval content (window.__bhRetry(), 'retried')
wait 4000
$H
cmd {"cmd":"open","input":"https://nonexistent.invalid/"}
wait 12000
$H
$U
shot err-dns
cmd {"cmd":"open","input":"http://127.0.0.1:9/"}
wait 4000
$H
shot err-restricted
cmd {"cmd":"navigate","input":"http://127.0.0.1:25/"}
wait 3000
$H
cmd {"cmd":"open","input":"http://127.0.0.1:47998/"}
wait 5000
$H
shot err-dropped
cmd {"cmd":"open","input":"http://127.0.0.1:47997/500"}
wait 4000
$H
shot err-500
cmd {"cmd":"open","input":"http://127.0.0.1:47997/403"}
wait 4000
$H
shot err-403
cmd {"cmd":"open","input":"https://127.0.0.1:47996/"}
wait 6000
$H
shot err-cert
cmd {"cmd":"open","input":"http://999.999.999.999/"}
wait 6000
eval content 'invalid ' + location.protocol
cmd {"cmd":"open","input":"http://127.0.0.1:47997/"}
wait 4000
eval content document.title
eval chrome 'alive'
quit
S
out=$(timeout 240 cargo run -q -p blackhole --features devtools 2>&1 | grep "dev\] eval ->" || true)
mapfile -t L <<<"$out"
fail=0
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
check 0 'Site injoignable' "refused: title shown"
check 1 '127.0.0.1' "refused: failed URL shown"
check 2 'function' "refused: retry available"
check 3 'retried' "refused: retry runs"
check 4 'Site injoignable' "refused: still the error page after a retry"
check 5 'Adresse introuvable' "dns: title shown"
check 6 'nonexistent.invalid' "dns: failed URL shown"
check 7 'Site injoignable' "restricted port: error page, not a blank tab"
check 8 'Site injoignable' "restricted port typed in an open tab: error page"
check 9 'Site injoignable' "connection dropped by the server: error page"
check 10 'Le serveur a un probl' "http 500 without a body: server error page"
check 11 'Accès refusé' "http 403 without a body: forbidden page"
check 12 'Connexion non sécurisée' "self-signed certificate: certificate page"
check 13 'invalid ' "invalid address: the tab is still usable"
check 14 'bh ok' "a working page loads after the errors"
check 15 'alive' "no crash: the app is still running"
for s in refused dns restricted dropped 500 403 cert; do
  [[ -f /tmp/bh_shots/err-$s.png ]] || { echo "FAIL screenshot err-$s missing"; fail=1; }
done
exit $fail
