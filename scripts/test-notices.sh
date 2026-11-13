#!/usr/bin/env bash
# Notices on macOS, driven by the devtools harness: with the sidebar open they are the sidebar's
# toast (icon of their kind, time line); with the sidebar hidden the top bar's address field
# becomes them (same component), buttons included; a notice follows the sidebar when it is shown
# or hidden meanwhile, the sidebar button keeps a dot while a card waits, and "animations off"
# is honoured. A local page only, no network.
# The app runs with a scratch HOME; profile /tmp/bh_data (wiped first); screenshots in
# /tmp/bh_shots/notice-*.png. Checks are read by name (`xx:`), not by index.
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
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        body = b"<!doctype html><title>Notices</title><h1>Notices</h1>"
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a):
        pass
ThreadingHTTPServer(("127.0.0.1", 47981), H).serve_forever()
P
python3 -I "$work/srv.py" & srv=$!
cargo build -q -p blackhole --features devtools
sleep 1

# What a host shows: shown|icon|tone|text|buttons|time line|time.
HOST='((h) => [h.classList.contains("show"), (h.querySelector(".nic svg") || {dataset: {}}).dataset.i, h.classList.contains("good") ? "good" : h.classList.contains("bad") ? "bad" : "plain", (h.querySelector(".txt") || {}).textContent, h.querySelectorAll("button").length, h.querySelector(".tbar") ? getComputedStyle(h.querySelector(".tbar")).animationName : "", h.style.getPropertyValue("--t")].join("|"))'
cat > /tmp/bh_script.txt <<S
wait 2000
cmd {"cmd":"set_language","language":"en"}
cmd {"cmd":"open","input":"http://127.0.0.1:47981/"}
wait 2000
cmd {"cmd":"shortcut","key":"zoom_in"}
wait 500
eval chrome 'o1:' + $HOST(document.getElementById('toast'))
eval bar 'o2:' + $HOST(document.getElementById('notice')) + '|' + document.body.classList.contains('waiting')
shot notice-open
wait 2400
eval chrome 'o3:' + document.getElementById('toast').classList.contains('show')
cmd {"cmd":"lock"}
wait 700
cmd {"cmd":"shortcut","key":"zoom_in"}
wait 90
eval bar 'h1:' + (() => { const o = +getComputedStyle(document.getElementById('notice')).opacity; return o > 0 && o < 1 ? 'between' : o; })()
wait 500
eval bar 'h2:' + $HOST(document.getElementById('notice')) + '|' + getComputedStyle(document.getElementById('notice')).opacity
shot notice-capsule
wait 2400
eval bar 'h3:' + document.getElementById('notice').classList.contains('show')
cmd {"cmd":"new_folder"}
wait 600
eval chrome '__f:' + (window.__f = state.folders.length) + (document.activeElement && document.activeElement.blur(), '')
eval chrome '__d:' + (send({ cmd: 'delete_folder', id: state.folders[state.folders.length - 1].id }), 'sent')
wait 700
eval bar 'u1:' + $HOST(document.getElementById('notice'))
shot notice-capsule-undo
cmd {"cmd":"lock"}
wait 700
eval bar 'u2:' + document.getElementById('notice').classList.contains('show')
eval chrome 'u3:' + $HOST(document.getElementById('toast'))
cmd {"cmd":"lock"}
wait 700
eval bar 'u4:' + document.getElementById('notice').classList.contains('show') + '|' + (parseFloat(document.querySelector('#notice .tbar').style.animationDelay) < -1000)
eval bar 'u5:' + (document.querySelector('#notice button').click(), 'undo')
wait 700
eval chrome 'u6:' + (state.folders.length - window.__f) + '|' + document.getElementById('toast').textContent
eval bar 'u7:' + (document.querySelector('#notice .txt') || {}).textContent + '|' + document.querySelectorAll('#notice button').length
wait 2400
beamtext Phone|Hello from the phone
wait 600
eval bar 's1:' + $HOST(document.getElementById('notice')) + '|' + document.body.classList.contains('waiting') + '|' + getComputedStyle(document.querySelector('#sidebar .dot')).transform
shot notice-capsule-two-buttons
cmd {"cmd":"lock"}
wait 700
eval bar 's2:' + document.body.classList.contains('waiting') + '|' + document.getElementById('notice').classList.contains('show')
eval chrome 's3:' + document.querySelectorAll('#signals .sg').length + '|' + document.getElementById('toast').classList.contains('show')
shot notice-open-card
cmd {"cmd":"lock"}
wait 8500
cmd {"cmd":"set_animations","on":false}
wait 500
cmd {"cmd":"shortcut","key":"zoom_reset"}
wait 60
eval bar 'n1:' + document.body.classList.contains('noanim') + '|' + getComputedStyle(document.getElementById('notice')).opacity + '|' + getComputedStyle(document.querySelector('#notice .tbar')).animationName
shot notice-capsule-noanim
cmd {"cmd":"set_animations","on":true}
eval chrome 'alive:yes'
quit
S
out=$(HOME="$work/home" timeout 180 target/debug/blackhole 2>&1 | grep "dev\] eval ->" || true)
fail=0
named() { grep -o "\"$1:[^\"]*" <<<"$out" | head -1 | cut -c2-; }
check() { local got; got=$(named "$1"); if [[ "$got" == $2 ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${got:-none})"; fail=1; fi; }
check o1 'o1:true|info|plain|Zoom*|0|toast-time|2200ms' "sidebar open: the toast shows the notice, its icon and its time line"
check o2 'o2:false|*|false' "sidebar open: the top bar shows nothing, no dot"
check o3 'o3:false' "sidebar open: the toast leaves after its time"
check h1 'h1:between' "sidebar hidden: the capsule is caught entering (intermediate state)"
check h2 'h2:true|info|plain|Zoom*|0|toast-time|2200ms|1' "sidebar hidden: the address field becomes the same notice"
check h3 'h3:false' "sidebar hidden: the capsule gives the address back"
check u1 'u1:true|history|plain|*deleted*|1|toast-time|8000ms' "sidebar hidden: a notice that asks keeps its button (Undo)"
check u2 'u2:false' "sidebar shown meanwhile: the capsule steps back"
check u3 'u3:true|history|plain|*deleted*|1|*' "sidebar shown meanwhile: the toast has the same notice"
check u4 'u4:true|true' "sidebar hidden again: the capsule takes over where the time is"
check u5 'u5:undo' "the capsule's button can be pressed"
check u6 'u6:0|*' "Undo from the capsule brings the folder back"
check u7 'u7:*|0' "the answered notice is replaced everywhere (no stale Undo)"
check s1 's1:true|devices|plain|*Hello*|2|toast-time|8000ms|true|none' "a text received: two buttons, and a dot on the sidebar button"
check s2 's2:false|false' "sidebar open: no dot, no capsule"
check s3 's3:1|true' "sidebar open: the card waits there, the toast goes on"
check n1 'n1:true|1|none' "animations off: the capsule is there at once, nothing moves"
check alive 'alive:yes' "no crash: the app is still running"
for s in notice-open notice-capsule notice-capsule-undo notice-capsule-two-buttons notice-open-card notice-capsule-noanim; do
  [[ -f /tmp/bh_shots/$s.png ]] || { echo "FAIL screenshot $s missing"; fail=1; }
done
exit $fail
