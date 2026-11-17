#!/usr/bin/env bash
# The 0.8.17 changes on macOS, driven by the devtools harness (captures /tmp/bh_shots/v0817-*):
# quick access tile sizes (small: 5 a row, medium: 4, large: 3), start page sections that can be
# hidden (watched areas and downloads only when there is something to show), and the floating
# sidebar kept while a menu opened from it (the space colour picker) is open, passkeys (honest
# WebAuthn answers, a notice when the system refuses a request).
# Local server on 127.0.0.1:47983. Scratch profile /tmp/bh_data (wiped first). Keep the window
# visible, with the pointer off it: the strip over the page hides the flyout as soon as it widens
# under a pointer that rests on the page (the flyout checks then fail).
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
        page = ("<!doctype html><title>Site %s</title><p>Site %s</p>" % (self.path, self.path)).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(page)))
        self.end_headers()
        self.wfile.write(page)
    def log_message(self, *a):
        pass
HTTPServer(("127.0.0.1", 47983), H).serve_forever()
P
python3 -I "$work/http.py" & pids+=($!)
sleep 1

# `qa`: columns of the quick access grid | height of a tile. `home`: now, favorites, watches,
# downloads, recent shown (1) or hidden (0). `state`: the flyout's visible width.
cat > /tmp/bh_script.txt <<'S'
wait 2500
cmd {"cmd":"set_language","language":"en"}
cmd {"cmd":"pin_site","url":"http://127.0.0.1:47983/a"}
cmd {"cmd":"pin_site","url":"http://127.0.0.1:47983/b"}
cmd {"cmd":"pin_site","url":"http://127.0.0.1:47983/c"}
cmd {"cmd":"pin_site","url":"http://127.0.0.1:47983/d"}
cmd {"cmd":"pin_site","url":"http://127.0.0.1:47983/e"}
cmd {"cmd":"pin_site","url":"http://127.0.0.1:47983/f"}
wait 2500
cmd {"cmd":"set_quick_access_size","size":"small"}
wait 600
eval chrome 'qa0:' + getComputedStyle($('pinned')).gridTemplateColumns.split(' ').length + '|' + Math.round(document.querySelector('#pinned .tab').getBoundingClientRect().height)
shot v0817-tiles-small
cmd {"cmd":"set_quick_access_size","size":"large"}
wait 600
eval chrome 'qa1:' + getComputedStyle($('pinned')).gridTemplateColumns.split(' ').length + '|' + Math.round(document.querySelector('#pinned .tab').getBoundingClientRect().height)
shot v0817-tiles-large
cmd {"cmd":"set_quick_access_size","size":"medium"}
wait 600
eval chrome 'qa2:' + getComputedStyle($('pinned')).gridTemplateColumns.split(' ').length + '|' + Math.round(document.querySelector('#pinned .tab').getBoundingClientRect().height)
cmd {"cmd":"home"}
wait 1500
eval home 'home0:' + ['now', 'favorites', 'watches', 'downloads', 'recent-card'].map(id => $(id).hidden ? 0 : 1).join('')
cmd {"cmd":"set_home_section","section":"recent","shown":false}
cmd {"cmd":"set_home_section","section":"favorites","shown":false}
wait 700
eval home 'home1:' + ['now', 'favorites', 'watches', 'downloads', 'recent-card'].map(id => $(id).hidden ? 0 : 1).join('')
shot v0817-home-hidden
eval home 'home2:' + (customizeMenu(300, 200), 1)
wait 800
shot v0817-home-menu
cmd {"cmd":"palette_close"}
cmd {"cmd":"set_home_section","section":"recent","shown":true}
cmd {"cmd":"set_home_section","section":"favorites","shown":true}
wait 700
eval home 'home3:' + ['now', 'favorites', 'watches', 'downloads', 'recent-card'].map(id => $(id).hidden ? 0 : 1).join('')
shot v0817-home
cmd {"cmd":"lock"}
wait 700
cmd {"cmd":"peek","on":true}
wait 700
state
eval flyout 'fly0:' + (spaceMenu(state.spaces[0], 60, 420), 1)
wait 500
cmd {"cmd":"peek","on":false}
wait 800
state
shot v0817-flyout-picker
cmd {"cmd":"palette_close"}
wait 300
cmd {"cmd":"peek","on":false}
wait 800
state
cmd {"cmd":"lock"}
wait 500
cmd {"cmd":"open","input":"http://localhost:47983/pk"}
wait 2000
eval content (window.__t0 = Date.now(), PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable().then(v => window.__u = v + ':' + (Date.now() - window.__t0)), PublicKeyCredential.getClientCapabilities().then(c => window.__k = c.passkeyPlatformAuthenticator + '|' + c.conditionalGet), navigator.credentials.get({ publicKey: { challenge: new Uint8Array(32), rpId: 'localhost', timeout: 5000 } }).catch(e => window.__g = e.name), 'pk0')
wait 1500
eval content 'pk1:' + window.__u + '|' + window.__k + '|' + window.__g
eval chrome 'pk2:' + [...document.querySelectorAll('.bh-notice')].some(n => n.textContent.includes('asked for a passkey'))
shot v0817-passkey-notice
quit
S

out=$(timeout 180 cargo run -q -p blackhole --features devtools 2>&1 | grep -E 'dev\] (eval|state) ->' || true)
mapfile -t L <<<"$out"
printf '%s\n' "${L[@]}"
fail=0
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
check 0 'qa0:5|' "small tiles: five a row"
check 1 'qa1:3|' "large tiles: three a row"
check 2 'qa2:4|48' "medium tiles: four a row, 48 px high (as before)"
h0=$([[ "${L[0]:-}" =~ qa0:5\|([0-9]+) ]] && echo "${BASH_REMATCH[1]}" || echo 0)
h1=$([[ "${L[1]:-}" =~ qa1:3\|([0-9]+) ]] && echo "${BASH_REMATCH[1]}" || echo 0)
if (( h0 > 0 && h0 < 48 && h1 > 48 )); then echo "ok   tile heights follow the size ($h0 < 48 < $h1)"; else echo "FAIL tile heights ($h0 / $h1)"; fail=1; fi
# Fresh profile: no watched area, no download, so those sections stay out of sight.
check 3 'home0:01001' "start page: favorites and recent shown, empty sections hidden"
check 4 'home1:00000' "start page: hidden sections stay hidden"
check 6 'home3:01001' "start page: sections shown again"
check 7 '"flyout":240' "hidden sidebar: the flyout comes out on hover"
check 9 '"flyout":240' "colour picker open: the flyout stays though the cursor left it"
check 10 '"flyout":0' "menu closed: the next move over the page hides the flyout"
# macOS: no platform authenticator, said at once (WebKit took seconds); the refusal is explained.
if [[ "${L[12]:-}" =~ pk1:false:([0-9]+)\|false\|false\|NotAllowedError ]] && (( BASH_REMATCH[1] < 200 )); then
  echo "ok   passkeys: no platform authenticator, said in ${BASH_REMATCH[1]} ms, request refused"
else echo "FAIL passkeys (got: ${L[12]:-none})"; fail=1; fi
check 13 'pk2:true' "passkeys: a notice explains the refused request"
for s in tiles-small tiles-large home-hidden home-menu home flyout-picker passkey-notice; do
  [[ -f /tmp/bh_shots/v0817-$s.png ]] || { echo "FAIL screenshot v0817-$s missing"; fail=1; }
done
exit $fail
