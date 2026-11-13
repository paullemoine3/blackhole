#!/usr/bin/env bash
# Downloads that start from a page already shown, on macOS, driven by the devtools harness: a
# "your download starts in a moment" page that sends itself to the file by script, then a link
# answered by a 302 to the file. The file is saved and the tab keeps its page: WebKit ends such
# a navigation as "frame load interrupted", which is not "Site unreachable". A local server
# only, no network.
# The app runs with a scratch HOME so the files land in a throwaway Downloads folder; profile
# /tmp/bh_data (wiped first); screenshot /tmp/bh_shots/download-redirect.png. Checks are read
# by name (`xx:`), not by index.
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
work=$(mktemp -d)
mkdir -p "$work/home/Downloads"
srv=
cleanup() { [[ -n "$srv" ]] && kill "$srv" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT
cat > "$work/srv.py" <<'P'
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
WAIT = b"""<!doctype html><title>Wait</title><h1>Downloading</h1>
<script>setTimeout(() => location.replace('/file/bh-sample.doc'), 400)</script>"""
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/hop":
            self.send_response(302)
            self.send_header("Location", "/file/bh-hop.doc")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        page = not self.path.startswith("/file/")
        body = WAIT if page else b"BlackHole test download\n" * 2000
        self.send_response(200)
        self.send_header("Content-Type", "text/html" if page else "application/msword")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a):
        pass
ThreadingHTTPServer(("127.0.0.1", 47983), H).serve_forever()
P
python3 -I "$work/srv.py" & srv=$!
cargo build -q -p blackhole --features devtools
sleep 1

PAGE="document.title + '|' + (document.querySelector('h1') || {}).textContent + '|' + location.pathname"
CARDS="document.querySelectorAll('#dls .dl').length + '|' + state.tabs.filter(t => (t.url || '').includes('47983')).length"
cat > /tmp/bh_script.txt <<S
wait 2000
cmd {"cmd":"set_language","language":"en"}
cmd {"cmd":"open","input":"http://127.0.0.1:47983/wait"}
wait 4000
eval content 'r1:' + $PAGE
eval chrome 'r2:' + $CARDS
shot download-redirect
eval content 'h0:' + (location.href = '/hop', 'asked')
wait 3000
eval content 'h1:' + $PAGE
eval chrome 'h2:' + $CARDS
eval chrome 'alive:yes'
quit
S
out=$(HOME="$work/home" timeout 120 target/debug/blackhole 2>&1 | grep "dev\] eval ->" || true)
fail=0
named() { grep -o "\"$1:[^\"]*" <<<"$out" | head -1 | cut -c2-; }
check() { local got; got=$(named "$1"); if [[ "$got" == $2 ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${got:-none})"; fail=1; fi; }
check r1 'r1:Wait|Downloading|*wait' "page that sends itself to the file: it stays shown (never \"Site unreachable\")"
check r2 'r2:1|1' "page that sends itself to the file: one download card, the tab stays"
check h1 'h1:Wait|Downloading|*wait' "link answered by a 302 to the file: the page stays shown"
check h2 'h2:2|1' "link answered by a 302 to the file: a second download card"
check alive 'alive:yes' "no crash: the app is still running"
for f in bh-sample.doc bh-hop.doc; do
  [[ -s "$work/home/Downloads/$f" ]] || { echo "FAIL $f did not land in the scratch Downloads"; fail=1; }
done
[[ -f /tmp/bh_shots/download-redirect.png ]] || { echo "FAIL screenshot download-redirect missing"; fail=1; }
exit $fail
