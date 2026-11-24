#!/usr/bin/env bash
# 0.8.29 scenarios on macOS (devtools harness): window size and user agent told to pages, undo and
# select-all keys (page first, then WebKit), private window (one shared session, private labels,
# no public window offered), long address inside its field.
# Local server on 127.0.0.1:47986. Scratch profile /tmp/bh_data (wiped first).
# The key checks need the app in front: they are skipped when macOS refuses to activate it.
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d)
pids=()
cleanup() { kill "${pids[@]}" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT
cat > "$work/http.py" <<'P'
from http.server import BaseHTTPRequestHandler, HTTPServer
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        if self.path == "/set":
            self.send_header("Set-Cookie", "bhpriv=1; Path=/")
        self.end_headers()
        self.wfile.write(b"<!doctype html><title>t</title><textarea id=t></textarea>")
    def log_message(self, *a):
        pass
HTTPServer(("127.0.0.1", 47986), H).serve_forever()
P
python3 -I "$work/http.py" & pids+=($!)
sleep 1
long="http://127.0.0.1:47986/$(printf 'very-long-segment-%.0s' $(seq 1 30))"
fail=0
run() { rm -rf /tmp/bh_data; timeout 120 cargo run -q -p blackhole --features devtools -- "$@" >"$work/log" 2>&1 || true; }
check() { if grep -qF -- "$1" "$work/log"; then echo "ok   $2"; else echo "FAIL $2"; fail=1; fi; }

{
echo 'wait 3000'
echo 'cmd {"cmd":"set_language","language":"en"}'
echo 'cmd {"cmd":"open","input":"http://127.0.0.1:47986/"}'
echo 'wait 3000'
echo "eval content 'size ' + (outerWidth === innerWidth && outerHeight === innerHeight && outerWidth > 0)"
echo "eval content 'agent ' + / Version\\/(2[6-9]|[3-9]\\d|1[89])\\.\\d+ Safari\\//.test(navigator.userAgent)"
echo "eval content (window.K = [], addEventListener('keydown', (e) => { K.push(e.key); if (window.block) e.preventDefault(); }), t.focus(), document.execCommand('insertText', false, 'abc'))"
for _ in 1 2 3 4 5; do echo 'press d'; echo 'wait 1200'; done
echo "eval content 'front ' + document.hasFocus()"
echo 'press a'; echo 'wait 700'
echo "eval content 'all ' + (K.includes('a') && t.selectionStart === 0 && t.selectionEnd === 3)"
echo 'press z'; echo 'wait 700'
echo "eval content 'undo ' + (K.includes('z') && t.value === '')"
echo 'press Z'; echo 'wait 700'
echo "eval content 'redo ' + (t.value === 'abc')"
echo "eval content (window.block = 1)"
echo 'press z'; echo 'wait 700'
echo "eval content 'kept ' + (t.value === 'abc')"
echo 'quit'
} > /tmp/bh_script.txt
run
check 'size true' 'page: outer size is the size of its card'
check 'agent true' 'page: Safari version of the engine'
if grep -q 'front true' "$work/log"; then
  check 'all true' 'keys: select all, seen by the page, done by WebKit'
  check 'undo true' 'keys: undo, seen by the page, done by WebKit'
  check 'redo true' 'keys: redo'
  check 'kept true' 'keys: an undo the page takes is left to it'
else
  echo 'skip keys: the app could not come to the front'
fi

{
echo 'wait 3000'
echo 'cmd {"cmd":"set_language","language":"en"}'
echo 'cmd {"cmd":"open","input":"http://127.0.0.1:47986/set"}'
echo 'wait 2500'
echo "cmd {\"cmd\":\"open\",\"input\":\"$long\"}"
echo 'wait 2500'
echo "eval content 'session ' + document.cookie"
echo "eval bar 'field ' + (() => { const o = document.getElementById('omni').getBoundingClientRect(), u = document.getElementById('url').getBoundingClientRect(); return u.left >= o.left && u.right <= o.right; })()"
echo "eval chrome document.getElementById('new').textContent"
echo 'shot private'
echo 'cmd {"cmd":"palette","mode":"new"}'
echo 'wait 800'
echo "eval palette 'public ' + document.body.innerText.includes('New window')"
echo 'quit'
} > /tmp/bh_script.txt
run --private
check 'session bhpriv=1' 'private: the tabs share one session'
check 'field true' 'private: a long address stays in its field'
check 'New private tab' 'private: new tabs say they are private'
check 'public false' 'private: no public window offered'

# Nothing of a private visit is left on disk, after a quit or a crash; the settings come along.
cat > /tmp/bh_script.txt <<'S'
wait 3000
cmd {"cmd":"open","input":"http://127.0.0.1:47986/ZQprivate"}
wait 5000
eval bar 'engine ' + JSON.stringify(last.engine)
wait 4000
quit
S
rm -rf /tmp/bh_data; mkdir -p /tmp/bh_data
# A settings file of the main profile, with another search engine.
echo 'wait 2000
cmd {"cmd":"set_engine","id":"duckduckgo"}
wait 2500
quit' > "$work/public.txt"
cp /tmp/bh_script.txt "$work/private.txt"; cp "$work/public.txt" /tmp/bh_script.txt
timeout 60 target/debug/blackhole >"$work/log" 2>&1 || true
cp "$work/private.txt" /tmp/bh_script.txt
touch "$work/mark"
timeout 60 target/debug/blackhole --private >"$work/log" 2>&1 || true
check 'DuckDuckGo' 'private: follows the settings of the profile'
traces() { find ~/Library/WebKit/blackhole ~/Library/Caches/blackhole ~/Library/HTTPStorages /tmp/bh_data "${TMPDIR:-/tmp}"/BlackHole-private-* -newer "$work/mark" -type f -print0 2>/dev/null | xargs -0 grep -laE 'ZQprivate|127\.0\.0\.1:47986' 2>/dev/null || true; }
if [[ -z "$(traces)" ]]; then echo 'ok   private: no trace on disk after a quit'; else echo 'FAIL private: traces left'; traces; fail=1; fi
target/debug/blackhole --private >/dev/null 2>&1 & victim=$!
sleep 7; kill -9 "$victim"; wait "$victim" 2>/dev/null || true
left="${TMPDIR:-/tmp}/BlackHole-private-$victim"
if [[ -d "$left" ]]; then echo 'ok   private: a crash leaves its folder (control)'; else echo 'FAIL private: no folder to sweep'; fail=1; fi
printf 'wait 3000\nquit\n' > /tmp/bh_script.txt
timeout 60 target/debug/blackhole >/dev/null 2>&1 || true
if [[ ! -d "$left" ]]; then echo 'ok   private: swept at the next launch'; else echo 'FAIL private: folder still there'; fail=1; fi

# A page in a call keeps its scripts running while its tab is hidden; not any more afterwards.
cat > /tmp/bh_script.txt <<'S'
wait 3000
cmd {"cmd":"open","input":"http://127.0.0.1:47986/call"}
wait 3000
eval content (async () => { window.n = 0; setInterval(() => n++, 20); window.a = new RTCPeerConnection(); window.b = new RTCPeerConnection(); a.onicecandidate = (e) => e.candidate && b.addIceCandidate(e.candidate); b.onicecandidate = (e) => e.candidate && a.addIceCandidate(e.candidate); a.createDataChannel('x'); await a.setLocalDescription(); await b.setRemoteDescription(a.localDescription); await b.setLocalDescription(); await a.setRemoteDescription(b.localDescription); })(), 0
wait 6000
cmd {"cmd":"open","input":"http://127.0.0.1:47986/other"}
wait 12000
eval tab2 (window.n1 = n, 0)
wait 5000
eval tab2 'hidden call ' + (document.visibilityState === 'hidden' && n - n1 > 80)
eval tab2 (a.close(), b.close(), 0)
wait 25000
eval tab2 (window.n2 = n, 0)
wait 5000
eval tab2 'after ' + (n - n2 < 80) + ' ' + (n - n2)
quit
S
run
check 'hidden call true' 'call: a hidden tab keeps running at speed'
check 'after true' 'call: throttled again once the call is over'
grep 'after' "$work/log" || true
exit $fail
