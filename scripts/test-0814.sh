#!/usr/bin/env bash
# The 0.8.14 changes on macOS, driven by the devtools harness (captures /tmp/bh_shots/v0814-*):
# anchored tab (mark, kept awake when the cap would put it to sleep), password manager switch,
# learned trackers card (allow / block again), starry start page, quick access without lean,
# start page tiles that rise without turning (0.8.16),
# address kept clear of the zoom badge, find in a long page (time and match), leaning cards calm
# at their edges and keeping the pointer, page card elevation.
# Local server on 127.0.0.1:47981 (a 30 000-line log). Scratch profile /tmp/bh_data (wiped first).
# Keep the window visible.
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
work=$(mktemp -d)
pids=()
cleanup() { kill "${pids[@]}" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT

cat > "$work/http.py" <<'P'
from http.server import BaseHTTPRequestHandler, HTTPServer
lines = "".join(f"<div class=l><span>{i:05d}</span> 2026-11-16T10:{i % 60:02d}:00Z <b>step</b> compiling crate number {i} of the workspace</div>" for i in range(30000))
LOG = ("<!doctype html><title>log</title><style>body{font:12px monospace}</style>" + lines).encode()
SMALL = b"<!doctype html><title>small</title><p>Small page</p>"
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        body = LOG if self.path == "/log" else SMALL
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except OSError:
            pass
    def log_message(self, *a):
        pass
HTTPServer(("127.0.0.1", 47981), H).serve_forever()
P
python3 -I "$work/http.py" & pids+=($!)
sleep 1

cat > /tmp/bh_script.txt <<'S'
wait 2500
cmd {"cmd":"set_language","language":"en"}
wait 400
eval home 'sky:' + (() => { const c = document.getElementById('sky'); const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data; let n = 0; for (let i = 3; i < d.length; i += 4) if (d[i]) n++; return (c.width > 0) + '|' + (n > 500); })()
eval home 'calm:' + (() => { const f = document.querySelector('.fav'); const r = f.getBoundingClientRect(); const at = (x, y, t) => (t || f).dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: x, clientY: y, pointerType: 'mouse' })); at(r.left + r.width * .2, r.top + r.height * .2); const up = f.classList.contains('bh-lean'); const flat = f.style.getPropertyValue('--ry') === '0' && f.style.getPropertyValue('--rx') === '0'; const m = new DOMMatrix(getComputedStyle(f).transform); const noTurn = Math.abs(m.m13) < 1e-6 && Math.abs(m.m23) < 1e-6; at(5, 5, document.body); return up + '|' + flat + '|' + noTurn + '|' + !f.classList.contains('bh-lean'); })()
shot v0814-home
cmd {"cmd":"open","input":"http://127.0.0.1:47981/small"}
wait 2500
eval chrome 'anch0:' + (() => { send({ cmd: 'tab_anchor', id: state.tabs[0].id }); return state.tabs.length; })()
wait 600
eval chrome 'anch:' + state.tabs[0].anchored + '|' + !!document.querySelector('.tab.anchored .anch svg.bh-i') + '|' + bhT('menu.anchor')
eval chrome 'pin0:' + (() => { send({ cmd: 'pin', id: state.tabs[0].id }); return 1; })()
wait 600
eval chrome 'pin:' + (() => { const t = document.querySelector('#pinned .tab'); const r = t.getBoundingClientRect(); t.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: r.left + r.width * .2, clientY: r.top + r.height * .2, pointerType: 'mouse' })); return !!t + '|' + !t.classList.contains('bh-lean') + '|' + t.classList.contains('anchored'); })()
cmd {"cmd":"set_max_live_tabs","tabs":2}
cmd {"cmd":"open","input":"http://127.0.0.1:47981/log"}
wait 3000
cmd {"cmd":"open","input":"http://127.0.0.1:47981/small?b"}
wait 1500
cmd {"cmd":"open","input":"http://127.0.0.1:47981/small?c"}
wait 1500
state
cmd {"cmd":"set_max_live_tabs","tabs":4}
cmd {"cmd":"open","input":"http://127.0.0.1:47981/log"}
wait 2500
eval content 'find:' + (() => { const r = document.createRange(); const n = document.querySelectorAll('.l')[25000].firstChild.firstChild; r.setStart(n, 0); r.setEnd(n, 5); getSelection().removeAllRanges(); getSelection().addRange(r); const t0 = performance.now(); __bhFind(); const t1 = performance.now(); __bhFind('next'); const t2 = performance.now(); return CSS.highlights.get('bh-find').size + '|' + Math.round(t1 - t0) + 'ms|' + Math.round(t2 - t1) + 'ms'; })()
eval content 'find2:' + (() => { const t0 = performance.now(); for (let i = 0; i < 5; i++) __bhFind('next'); return Math.round(performance.now() - t0) + 'ms'; })()
shot v0814-find
cmd {"cmd":"shortcut","key":"zoom_in"}
cmd {"cmd":"shortcut","key":"zoom_in"}
wait 700
eval bar 'zoom:' + document.body.classList.contains('zoomed') + '|' + (() => { const u = document.getElementById('url').getBoundingClientRect(), z = document.getElementById('zoom').getBoundingClientRect(); return u.right <= z.left + 1; })()
shot v0814-zoom
cmd {"cmd":"shortcut","key":"zoom_reset"}
eval chrome 'card:' + document.body.classList.contains('flat') + '|' + getComputedStyle(document.getElementById('cardshadow'), '::before').boxShadow.split(/\),|px,/).length
cmd {"cmd":"open_settings","tab":"passwords"}
wait 1200
eval settings 'pw:' + !!document.querySelector('#pwmanager .opt.sel') + '|' + document.querySelector('#pwmanager .label').textContent
cmd {"cmd":"passwords","action":{"op":"enabled","on":false}}
wait 500
eval settings 'pw2:' + !document.querySelector('#pwmanager .opt.sel')
shots v0814-passwords
cmd {"cmd":"passwords","action":{"op":"enabled","on":true}}
cmd {"cmd":"open_settings","tab":"privacy"}
wait 1000
eval settings 'trk:' + !!document.querySelector('#learning .opt.sel') + '|' + !!document.querySelector('#learnedlist .empty') + '|' + !!document.querySelector('#allowedlist .empty')
cmd {"cmd":"tracker_allow","host":"www.spy.example"}
wait 500
eval settings 'trk2:' + [...document.querySelectorAll('#allowedlist .host')].map(n => n.textContent).join(',')
shots v0814-privacy
cmd {"cmd":"tracker_block","host":"spy.example"}
wait 500
eval settings 'trk3:' + [...document.querySelectorAll('#learnedlist .host')].map(n => n.textContent).join(',') + '|' + !!document.querySelector('#allowedlist .empty')
state
cmd {"cmd":"trackers_forget"}
wait 300
cmd {"cmd":"close_settings"}
wait 500
shot v0814-sidebar
quit
S

out=$(timeout 240 cargo run -q -p blackhole --features devtools 2>&1 | grep -E 'dev\] (eval|state) ->' || true)
mapfile -t L <<<"$out"
fail=0
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
check 0 'sky:true|true' "start page: a sky of stars is drawn"
check 1 'calm:true|true|true|true' "a tile rises under the pointer without turning (0.8.16), rests when left"
check 3 'anch:true|true|Anchor' "anchored tab: state, anchor mark, menu entry"
check 5 'pin:true|true|true' "quick access tile does not lean, stays anchored"
check 6 '"anchoredAlive":1' "the anchored tab stays awake past the cap on live tabs"
check 7 'find:2|' "find: the selection is found in a 30 000-line page (its line and its text)"
if [[ "${L[7]:-}" =~ find:2\|([0-9]+)ms ]] && (( BASH_REMATCH[1] < 1500 )); then echo "ok   find reads the long page in ${BASH_REMATCH[1]} ms"; else echo "FAIL find too slow (got: ${L[7]:-none})"; fail=1; fi
check 9 'zoom:true|true' "the address keeps clear of the zoom badge"
check 10 'card:false|' "page card shadow read (3D style)"
check 11 'pw:true|Offer and save passwords' "password manager switch on by default"
check 12 'pw2:true' "password manager switched off"
check 13 'trk:true|true|true' "learned trackers: switch on, nothing learned, nothing allowed"
check 14 'trk2:spy.example' "a host allowed is listed (www. dropped)"
check 15 'trk3:spy.example|true' "blocked again: back with the learned trackers"
check 16 '"allowed":0' "state: nothing allowed any more"
for s in home find zoom sidebar; do
  [[ -f /tmp/bh_shots/v0814-$s.png ]] || { echo "FAIL screenshot v0814-$s missing"; fail=1; }
done
printf '%s\n' "${L[@]}" | sed -n '7,9p;11p'
exit $fail
