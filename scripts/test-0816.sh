#!/usr/bin/env bash
# The 0.8.16 changes on macOS, driven by the devtools harness (captures /tmp/bh_shots/v0816-*):
# Web Inspector docked in a page stays inside the page card (top bar and sidebar kept), the page
# comes back to its card when it is detached or closed; the colour picker of a space (presets,
# hex value, pad by keyboard) colours the space while the menu stays open; a new tab keeps the
# sidebar rows and their icons (no flicker).
# Local server on 127.0.0.1:47982. Scratch profile /tmp/bh_data (wiped first). Keep the window
# visible.
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
work=$(mktemp -d)
pids=()
cleanup() { kill "${pids[@]}" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT

cat > "$work/http.py" <<'P'
from http.server import BaseHTTPRequestHandler, HTTPServer
PAGE = b"<!doctype html><title>inspected</title><style>html,body{margin:0;height:100%;background:#e8f5e9}</style><p>Inspected page</p>"
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(PAGE)))
        self.end_headers()
        self.wfile.write(PAGE)
    def log_message(self, *a):
        pass
HTTPServer(("127.0.0.1", 47982), H).serve_forever()
P
python3 -I "$work/http.py" & pids+=($!)
sleep 1

# `card:` the page's own size against the content area, before, docked, after closing.
cat > /tmp/bh_script.txt <<'S'
wait 2500
cmd {"cmd":"set_language","language":"en"}
cmd {"cmd":"open","input":"http://127.0.0.1:47982/"}
wait 2500
eval content 'card0:' + innerWidth + 'x' + innerHeight
shot v0816-page
inspect show
wait 2000
inspect attach
wait 2000
eval content 'card1:' + innerWidth + 'x' + innerHeight
eval bar 'bar1:' + (document.visibilityState) + '|' + innerWidth
shot v0816-docked
inspect close
wait 1500
eval content 'card2:' + innerWidth + 'x' + innerHeight
shot v0816-closed
inspect show
wait 2000
inspect detach
wait 1500
eval content 'card3:' + innerWidth + 'x' + innerHeight
shot v0816-detached
inspect close
wait 800
eval chrome 'pick0:' + (spaceMenu(state.spaces[0], 120, 560), state.spaces[0].color)
wait 900
eval palette 'pick1:' + !!document.querySelector('#menu .cpick') + '|' + document.querySelectorAll('#menu .cpresets button').length + '|' + document.querySelector('#menu .chex input').value
eval palette 'pick2:' + (() => { const i = document.querySelector('#menu .chex input'); i.value = '#12a4b6'; i.dispatchEvent(new Event('input')); return i.value; })()
wait 600
eval chrome 'pick3:' + state.spaces[0].color
eval palette 'pick4:' + document.body.classList.contains('menuing') + '|' + (() => { const p = document.querySelector('#menu .cpad'); p.focus(); p.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', shiftKey: true, bubbles: true })); return document.querySelector('#menu .chex input').value; })()
wait 600
shot v0816-picker
eval chrome 'pick5:' + state.spaces[0].color
eval palette 'pick6:' + (document.querySelectorAll('#menu .cpresets button')[1].click(), 1)
wait 900
eval chrome 'pick7:' + state.spaces[0].color
eval palette 'pick8:' + document.body.classList.contains('menuing')
cmd {"cmd":"open","input":"http://127.0.0.1:47982/b"}
wait 2000
eval chrome 'rows0:' + (window.__rows = [...document.querySelectorAll('#scroll .tab[data-tab]')], window.__dots = [...document.querySelectorAll('#scroll .tab[data-tab] .dot')], window.__rows.length)
cmd {"cmd":"open","input":"http://127.0.0.1:47982/c"}
wait 2500
eval chrome 'rows1:' + window.__rows.filter(n => n.isConnected).length + '|' + window.__dots.filter(n => n.isConnected).length + '/' + window.__dots.length
quit
S

out=$(timeout 180 cargo run -q -p blackhole --features devtools 2>&1 | grep -E 'dev\] (eval|state) ->' || true)
mapfile -t L <<<"$out"
printf '%s\n' "${L[@]}"
fail=0
size() { [[ "${L[$1]:-}" =~ card[0-9]:([0-9]+)x([0-9]+) ]] && echo "${BASH_REMATCH[1]} ${BASH_REMATCH[2]}"; }
read -r w0 h0 <<<"$(size 0)"
read -r w1 h1 <<<"$(size 1)"
read -r w2 h2 <<<"$(size 3)"
read -r w3 h3 <<<"$(size 4)"
if [[ -n "${w0:-}" && -n "${w1:-}" ]] && (( w1 <= w0 && h1 < h0 )); then
  echo "ok   docked inspector takes room inside the card (${w0}x${h0} -> ${w1}x${h1})"
else
  echo "FAIL docked inspector grew the page out of its card (${L[0]:-none} -> ${L[1]:-none})"; fail=1
fi
if [[ "$w2 $h2" == "$w0 $h0" ]]; then echo "ok   closed: the page has its card back"; else echo "FAIL closed: ${L[3]:-none}"; fail=1; fi
if [[ "$w3 $h3" == "$w0 $h0" ]]; then echo "ok   detached: the page has its card back"; else echo "FAIL detached: ${L[4]:-none}"; fail=1; fi
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
check 6 'pick1:true|8|#' "space menu: a colour picker with the 8 presets and the hex value"
check 8 'pick3:#12a4b6' "a typed hex colour reaches the space at once"
check 9 'pick4:true|#' "the menu stays open while the colour is chosen"
if [[ "${L[9]:-}" != *'#12a4b6'* && "${L[10]:-}" == *"${L[9]##*|}"* ]]; then echo "ok   the pad moves the hue by keyboard, the space follows"; else echo "FAIL pad by keyboard (${L[9]:-none} / ${L[10]:-none})"; fail=1; fi
check 12 'pick7:#ff5c8a' "a preset applies"
check 13 'pick8:false' "a preset closes the menu"
# A new tab keeps the other rows (only the one that stops being active is redrawn) and every icon.
if [[ "${L[14]:-}" =~ rows0:([0-9]+) ]] && n=${BASH_REMATCH[1]} && [[ "${L[15]:-}" == *"rows1:$((n - 1))|$n"*"/$n"* ]]; then
  echo "ok   a new tab keeps the sidebar rows and their icons ($n rows)"
else echo "FAIL sidebar rows rebuilt on a new tab (${L[14]:-none} / ${L[15]:-none})"; fail=1; fi
for s in page docked closed detached picker; do
  [[ -f /tmp/bh_shots/v0816-$s.png ]] || { echo "FAIL screenshot v0816-$s missing"; fail=1; }
done
exit $fail
