#!/usr/bin/env bash
# 0.8.34 scenarios on macOS (devtools harness): a tab dragged out of the sidebar stays in sight.
# Over a page, the half under the pointer slides in and takes it (split view); over the start
# page, the favorites take it; outside the window, a small window follows the pointer and says
# what a drop would do (Escape cancels: no window is opened). On the start page only the Favorites
# section takes it (y 210 of the sidebar view: in that section at the default window size). The drag is made of synthetic
# mouse events in the sidebar (its view spans the window: screen points from window.screenX /
# screenY). It is never released outside: a drop there would open a window (dev builds open
# three at most, and a window opened by another never replays the script).
# Local server on 127.0.0.1:47989. Scratch profile /tmp/bh_data (wiped first). Keep the window
# visible; screenshots in /tmp/bh_shots/v0834-*.
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
work=$(mktemp -d)
pids=()
cleanup() { kill "${pids[@]}" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT
cat > "$work/http.py" <<'P'
from http.server import BaseHTTPRequestHandler, HTTPServer
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        body = ("<!doctype html><title>Page %s</title><p>Page %s" % (self.path, self.path)).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a):
        pass
HTTPServer(("127.0.0.1", 47989), H).serve_forever()
P
python3 -I "$work/http.py" & pids+=($!)
# Never more than three dev windows: past that, all of them are stopped.
( while sleep 0.3; do
    n=$(pgrep -f "target/debug/blackhole" | wc -l)
    if (( n > 3 )); then pkill -9 -f "target/debug/blackhole"; rm -f /tmp/bh_script.txt; echo "guard: $n windows, stopped" >&2; fi
  done ) & pids+=($!)
sleep 1
site=http://127.0.0.1:47989
# dragTab(path): presses the tab of that page and moves it past the threshold; dragMove(f): to
# the page at fraction f of its width (out: past the window's right edge); dragUp / dragEsc.
helpers="window.idOf = (p) => state.tabs.find(t => t.url.endsWith(p)).id; window.ev = (el, type, x, y) => el.dispatchEvent(new MouseEvent(type, {button: 0, clientX: x, clientY: y, screenX: screenX + x, screenY: screenY + y, bubbles: true, cancelable: true})); window.dragTab = (p) => { const n = document.querySelector('#scroll [data-tab=\"' + idOf(p) + '\"]'), r = n.getBoundingClientRect(); ev(n, 'mousedown', r.left + 20, r.top + 8); ev(document, 'mousemove', r.left + 34, r.top + 8); return 'pressed'; }; window.dragMove = (f) => { const side = document.querySelector('aside').getBoundingClientRect().right; window.at = [f === 'out' ? outerWidth + 220 : side + (outerWidth - side) * f, innerHeight / 2]; ev(document, 'mousemove', at[0], at[1]); return 'moved'; }; window.dragOnto = (p) => { const r = document.querySelector('#scroll [data-tab=\"' + idOf(p) + '\"]').getBoundingClientRect(); window.at = [r.left + 12, r.top + r.height / 2]; ev(document, 'mousemove', r.left + 20, r.top + 6); ev(document, 'mousemove', at[0], at[1]); return 'onto'; }; window.dragUp = () => (ev(document, 'mouseup', at[0], at[1]), 'up'); window.dragEsc = () => (document.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true})), 'esc'); window.panes = () => state.tabs.filter(t => t.split).map(t => t.url.slice(-1)).sort().join(',') + '>' + (state.tabs.find(t => t.id === state.activeTab) || {url: '-'}).url.slice(-1); 'helpers'"
look="'|' + document.body.className.trim() + '|' + document.getElementById('pane').className + '|' + document.getElementById('panetxt').textContent + '|' + document.getElementById('favtxt').textContent + '|' + document.getElementById('fav').className + '|' + document.getElementById('badge').textContent"
{
echo 'wait 3000'
echo 'cmd {"cmd":"set_language","language":"en"}'
for p in a b c; do echo "cmd {\"cmd\":\"open\",\"input\":\"$site/$p\"}"; echo 'wait 1200'; done
echo "eval chrome $helpers"
# Over the left half of the page: it slides in, then the right half.
echo "eval chrome (dragTab('/a'), dragMove(0.25))"
echo 'wait 700'
echo "eval drag 'z1 ' + $look"
echo 'shot v0834-split-left'
echo "eval chrome dragMove(0.8)"
echo 'wait 500'
echo "eval drag 'z2 ' + $look"
echo "eval chrome dragUp()"
echo 'wait 140'
echo 'shot v0834-pane-drop'
echo 'wait 1060'
echo "eval chrome 'sp1 ' + panes()"
echo "eval drag 'z3 ' + document.body.classList.contains('on')"
# A split: the tab dropped on the left pane takes it.
echo "eval chrome (dragTab('/b'), dragMove(0.2))"
echo 'wait 500'
echo "eval drag 'z4 ' + $look"
echo "eval chrome dragUp()"
echo 'wait 1200'
echo "eval chrome 'sp2 ' + panes()"
echo 'shot v0834-split-done'
# The start page: the favorites take it, once.
echo 'cmd {"cmd":"home"}'
echo 'wait 1200'
echo "eval chrome (dragTab('/c'), dragMove(0.5))"
echo 'wait 600'
echo "eval drag 'f0 ' + document.body.classList.contains('fav')"
echo "eval home (window.fr = document.getElementById('favorites').getBoundingClientRect(), 'fav ' + Math.round(fr.top))"
echo "eval chrome (window.at = [at[0], 210], ev(document, 'mousemove', at[0], at[1]), 'over favorites')"
echo 'wait 600'
echo "eval drag 'f1 ' + $look"
echo 'shot v0834-favorite'
echo "eval chrome dragUp()"
echo 'wait 1200'
echo "eval chrome 'f2 ' + state.tabs.find(t => t.url.endsWith('/c')).pinned"
echo "eval home 'f5 ' + !!document.querySelector('#favs .fav.arrive')"
echo "eval chrome (dragTab('/c'), dragMove(0.5), ev(document, 'mousemove', at[0], 210), window.at = [at[0], 210], 'moved')"
echo 'wait 500'
echo "eval drag 'f3 ' + $look"
echo "eval chrome dragEsc()"
echo 'wait 400'
echo "eval drag 'f4 ' + document.body.classList.contains('on')"
# In the sidebar: the row glides from where its ghost was let go, and lights up.
echo "eval chrome (dragTab('/a'), dragOnto('/c'), window.lean0 = getComputedStyle(document.querySelector('.ghost')).backdropFilter, dragUp())"
echo 'wait 120'
echo "eval chrome 'l1 ' + !!document.querySelector('#scroll .tab.landed') + '|' + /blur/.test(lean0)"
echo 'wait 900'
echo "eval chrome 'l2 ' + !!document.querySelector('#scroll .tab.landed')"
# Outside the window: a window of its own follows, and says a new window would open.
echo "eval chrome (dragTab('/a'), dragMove('out'))"
echo 'wait 2000'
echo "eval chrome dragMove('out')"
echo 'wait 600'
echo 'state'
echo "eval dragfloat 'o1 ' + $look"
echo "eval chrome dragMove(0.5)"
echo 'wait 500'
echo 'state'
echo "eval chrome dragEsc()"
echo 'wait 600'
echo 'state'
echo "eval chrome 'o2 ' + state.tabs.length"
echo 'quit'
} > /tmp/bh_script.txt
cargo build -q -p blackhole --features devtools
timeout 180 ./target/debug/blackhole >"$work/log" 2>&1 || true; cp "$work/log" /tmp/bh_0834.log
grep -E 'dev\] (eval|state) ->' "$work/log" || true
fail=0
check() { if grep -qF -- "$1" "$work/log"; then echo "ok   $2"; else echo "FAIL $2"; fail=1; fi; }
check 'z1 |on|left shown|Open on the left|' 'over a page: the left half slides in'
check 'z2 |on|right shown|Open on the right|' 'over a page: then the right half'
check 'sp1 a,c>c' 'dropped on the right: split view with the page shown on the left'
check 'z3 false' 'dropped: nothing left drawn above the page'
check 'z4 |on|left shown|Show in this pane|' 'split view: the left pane would take it'
check 'sp2 a,b>b' 'dropped on the left pane: it replaces it'
check 'f0 false' 'start page: elsewhere than the Favorites section, nothing'
check 'f1 |on fav|' 'start page: the Favorites section would take it'
check '|Add to favorites||Add to favorites' 'start page: says it adds it to the favorites'
check 'f2 true' 'dropped on the start page: in the favorites (quick access)'
check '|Already in favorites|no|' 'start page: already a favorite, said'
check 'f4 false' 'Escape: the drawing ends'
check 'f5 true' 'start page: the new favorite settles in with a ring'
check 'l1 true|true' 'sidebar: a glass ghost, the row lands lit where it was dropped'
check 'l2 false' 'sidebar: the light fades'
check 'o1 |float on||' 'outside: the window of its own shows the tab'
check 'New window' 'outside: says a new window would open'
check '"dragFloat":[-20000.0,-20000.0]' 'back over the window: the window outside steps away'
check 'o2 3' 'Escape outside: no window opened, the tab stays'
grep -qF '"dragFloat":[' "$work/log" && grep -qF '"dragFloat":null' "$work/log" && echo 'ok   Escape: the window outside is gone' || { echo 'FAIL Escape: the window outside is gone'; fail=1; }
for s in split-left pane-drop split-done favorite; do
  [[ -f /tmp/bh_shots/v0834-$s.png ]] || { echo "FAIL screenshot v0834-$s missing"; fail=1; }
done
exit $fail
