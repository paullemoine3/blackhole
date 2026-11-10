#!/usr/bin/env bash
# Native request blocking and media probing on macOS, driven by the devtools harness.
# 1. Trackers are blocked by WebKit content rule lists (no script in the page): third-party
#    requests to listed hosts fail and are counted, another third-party host still loads, and
#    the page keeps its native `setAttribute` / `XMLHttpRequest` (`fetch` is not checked: debug
#    builds wrap it for their log, see `ui/debug_log.js`).
# 2. The shell only probes the media of tabs that said something changed: a playing sound is
#    seen, a paused one is seen too, and the tab is left alone a few seconds later.
# Local server on 127.0.0.1:47985 (also reached as `localhost`, a third party of the page).
# The two blocked requests never leave the machine. Scratch profile /tmp/bh_data (wiped first).
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
work=$(mktemp -d)
pids=()
cleanup() { kill "${pids[@]}" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT

cat > "$work/http.py" <<'P'
import io, wave
from http.server import BaseHTTPRequestHandler, HTTPServer
tone = io.BytesIO()
with wave.open(tone, "wb") as w:
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(8000)
    w.writeframes(b"\x00\x00" * 8000 * 60)
PAGE = b"""<!doctype html><title>blocking</title><audio src="/tone.wav"></audio><script>
window.r = {};
fetch('https://www.google-analytics.com/collect?v=1', {mode: 'no-cors'}).then(() => r.tracker = 'loaded', () => r.tracker = 'blocked');
fetch('http://localhost:47985/ok', {mode: 'no-cors'}).then(() => r.other = 'loaded', () => r.other = 'blocked');
const i = new Image(); i.onload = () => r.img = 'loaded'; i.onerror = () => r.img = 'blocked';
i.src = 'https://stats.g.doubleclick.net/r/collect';
</script>"""
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        body, kind = (tone.getvalue(), "audio/wav") if self.path == "/tone.wav" else (PAGE if self.path == "/page" else b"ok", "text/html")
        self.send_response(200)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except OSError:
            pass
    def log_message(self, *a):
        pass
HTTPServer(("127.0.0.1", 47985), H).serve_forever()
P
python3 -I "$work/http.py" & pids+=($!)
sleep 1

cat > /tmp/bh_script.txt <<'S'
wait 2000
cmd {"cmd":"set_language","language":"en"}
wait 300
cmd {"cmd":"open","input":"http://127.0.0.1:47985/page"}
wait 4000
eval content 'tracker ' + r.tracker
eval content 'img ' + r.img
eval content 'other ' + r.other
eval content 'native ' + [Element.prototype.setAttribute, XMLHttpRequest.prototype.open, XMLHttpRequest.prototype.send].every((f) => String(f).includes('[native code]'))
state
wait 6000
state
eval content (document.querySelector('audio').play(), 'play')
wait 3000
state
eval content (document.querySelector('audio').pause(), 'pause')
wait 3000
state
wait 6000
state
eval chrome 'alive'
quit
S
log=/tmp/bh_blocking.log
timeout 240 cargo run -q -p blackhole --features devtools >"$log" 2>&1 || true
mapfile -t L < <(grep "dev\] eval ->" "$log" || true)
mapfile -t S < <(grep "dev\] state ->" "$log" || true)
fail=0
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
state() { if [[ "${S[$1]:-}" =~ $2 ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${S[$1]:-none})"; fail=1; fi; }
check 0 'tracker blocked' "blocking: fetch to a listed tracker fails"
check 1 'img blocked' "blocking: image from a listed ad server fails"
check 2 'other loaded' "blocking: another third-party host still loads"
check 3 'native true' "blocking: the page keeps its native functions"
state 0 '"blocked":[2-9]' "blocking: blocked requests are counted"
state 1 '"mediaHints":0' "media: an idle tab is no longer probed"
state 2 '"mediaPlaying":1' "media: a sound that starts is seen"
state 3 '"mediaPlaying":0' "media: a pause is seen"
state 4 '"mediaHints":0' "media: the tab is left alone again"
check 6 'alive' "no crash: the app is still running"
if grep -q "content rule list" "$log"; then echo "FAIL a content rule list was refused"; grep "content rule list" "$log"; fail=1; fi
exit $fail
