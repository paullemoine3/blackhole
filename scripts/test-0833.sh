#!/usr/bin/env bash
# 0.8.33 scenarios on macOS (devtools harness): a Pulsar dragged to the folders and back, a start
# page per space (its favorites, sections and favorites order), the live tabs cap held per space,
# the screen share hints of WebKit installed in pages (a real Meet call is not covered), and the
# watched areas of the start page read again on demand (last check shown, area not found), the
# highlight of the active tab on a switch and a close, and the find bar keeping the keyboard.
# Local server on 127.0.0.1:47988. Scratch profile /tmp/bh_data (wiped first). Keep the window
# visible; screenshots in /tmp/bh_shots/v0833-*.
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
work=$(mktemp -d)
pids=()
cleanup() { kill "${pids[@]}" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT
cat > "$work/http.py" <<'P'
from http.server import BaseHTTPRequestHandler, HTTPServer
FEED = b"""<?xml version="1.0"?><rss version="2.0"><channel><title>Local news</title><link>http://127.0.0.1:47988/</link>
<item><title>First post</title><link>http://127.0.0.1:47988/p1</link><guid>p1</guid></item>
<item><title>Second post</title><link>http://127.0.0.1:47988/p2</link><guid>p2</guid></item>
</channel></rss>"""
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        feed = self.path == "/feed.xml"
        body = FEED if feed else ("<!doctype html><title>Page %s</title><p id=p>Page %s" % (self.path, self.path)).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/rss+xml" if feed else "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a):
        pass
HTTPServer(("127.0.0.1", 47988), H).serve_forever()
P
python3 -I "$work/http.py" & pids+=($!)
sleep 1
site=http://127.0.0.1:47988
# Two watched areas read a minute ago (not due before 30 min): one whose text has changed since,
# one whose element is gone. Only "Check all now" reads them again.
mkdir -p /tmp/bh_data
ago=$(( $(date +%s) - 60 ))
cat > /tmp/bh_data/watches.json <<J
[{"id":1,"url":"$site/w","selector":"#p","title":"Found","value":"Old text","checked_at":$ago},
 {"id":2,"url":"$site/w","selector":"#gone","title":"Gone","value":"Was here","checked_at":$ago}]
J
# `drag`: a Pulsar header pressed, moved past the threshold, released above or below the line
# under the folders (`up`).
drag="window.dragPulsar = (up) => { const h = document.querySelector('.ph'), r = h.getBoundingClientRect(), sep = document.getElementById('foldsep').getBoundingClientRect(), tabs = document.getElementById('tabs').getBoundingClientRect(); const y = up ? sep.top - 6 : tabs.bottom - 4, x = r.left + 30; const ev = (n, t, cx, cy) => n.dispatchEvent(new MouseEvent(t, {button: 0, clientX: cx, clientY: cy, bubbles: true, cancelable: true})); ev(h, 'mousedown', x, r.top + 5); ev(document, 'mousemove', x, r.top + 16); ev(document, 'mousemove', x, y); ev(document, 'mouseup', x, y); return 1; }"
sites="[...document.getElementById('favs').children].map(n => n.dataset.site)"
{
echo 'wait 3000'
echo 'cmd {"cmd":"set_language","language":"en"}'
echo 'cmd {"cmd":"set_max_live_tabs","tabs":2}'
echo "cmd {\"cmd\":\"pin_site\",\"url\":\"$site/a\"}"
echo 'wait 1500'
echo "cmd {\"cmd\":\"open\",\"input\":\"$site/b\"}"
echo 'wait 1200'
echo "cmd {\"cmd\":\"open\",\"input\":\"$site/c\"}"
echo 'wait 1500'
# WebKit offers no capture to a plain http page (127.0.0.1 included): checked only when it does.
echo "eval content 'meet ' + (navigator.mediaDevices ? /screens\\.add/.test(String(navigator.mediaDevices.getDisplayMedia)) + '|' + RTCRtpSender.prototype.setParameters.name : 'no capture')"
echo "cmd {\"cmd\":\"pulsar_new\",\"input\":\"$site/feed.xml\"}"
echo 'wait 3000'
echo "eval chrome ($drag, 'pul0 ' + document.querySelectorAll('#tabs .pulsar').length)"
echo "eval chrome (dragPulsar(true), 'dragged')"
echo 'wait 900'
echo "eval chrome 'pul1 ' + document.querySelectorAll('#folders .pulsar').length + '|' + document.querySelectorAll('#tabs .pulsar').length"
echo 'shot v0833-pulsar-folders'
echo "eval chrome (dragPulsar(false), 'dragged')"
echo 'wait 900'
echo "eval chrome 'pul2 ' + document.querySelectorAll('#folders .pulsar').length + '|' + document.querySelectorAll('#tabs .pulsar').length"
echo 'cmd {"cmd":"set_home_section","section":"recent","shown":false}'
echo 'cmd {"cmd":"home"}'
echo 'wait 1500'
echo "eval home 'home1 ' + $sites.includes('127.0.0.1') + '|' + document.getElementById('recent-card').hidden"
echo "eval home (window.first = $sites[0], document.getElementById('favs').children[0].dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowRight', altKey: true, bubbles: true})), 'moved')"
echo 'wait 900'
echo "eval home 'home2 ' + ($sites[1] === first)"
echo "eval home 'wat0 ' + document.querySelectorAll('#watchlist .item.checking').length + '|' + document.getElementById('watchlist').textContent.includes('checked 1 min ago') + '|' + document.getElementById('checkall').disabled"
echo "eval home (document.getElementById('checkall').click(), 'checkall')"
echo 'wait 1500'
echo "eval home 'wat1 ' + document.getElementById('checkall').disabled + '|' + document.querySelectorAll('#watchlist .item.checking').length + '|' + getComputedStyle(document.querySelector('#watchlist .item.checking'), '::before').animationName"
echo 'shot v0833-watch-checking'
echo 'wait 26000'
echo "eval home 'wat2 ' + document.getElementById('checkall').disabled + '|' + document.getElementById('watchlist').textContent.includes('area not found') + '|' + document.getElementById('watchlist').textContent.includes('checked just now') + '|' + document.getElementById('watchlist').textContent.includes('Page /w')"
echo 'shot v0833-watch-checked'
echo 'cmd {"cmd":"watch_check","id":1}'
echo 'wait 1200'
echo "eval home 'wat3 ' + document.querySelectorAll('#watchlist .item.checking').length"
echo 'shot v0833-home-first'
echo 'cmd {"cmd":"new_space","name":"Work","color":"#3b82f6"}'
echo 'wait 1200'
echo 'cmd {"cmd":"home"}'
echo 'wait 1500'
echo "eval home 'home3 ' + $sites.includes('127.0.0.1') + '|' + document.getElementById('recent-card').hidden + '|' + ($sites[0] === first)"
echo 'shot v0833-home-second'
echo "cmd {\"cmd\":\"open\",\"input\":\"$site/d\"}"
echo 'wait 1200'
echo "cmd {\"cmd\":\"open\",\"input\":\"$site/e\"}"
echo 'wait 1200'
echo "cmd {\"cmd\":\"open\",\"input\":\"$site/f\"}"
echo 'wait 2000'
echo 'state'
# The highlight of the active tab: it glides onto the tab chosen; the tab shown closed, none.
echo "eval chrome (window.glides = [], new MutationObserver(ms => { for (const m of ms) for (const n of m.addedNodes) if (n.classList && n.classList.contains('tab-glide')) { glides.push('new'); n.getAnimations()[0].finished.then(() => glides.push(Math.round(n.getBoundingClientRect().top - document.querySelector('#scroll .tab.active').getBoundingClientRect().top))); } }).observe(document.body, {childList: true}), window.__bhipc.postMessage(JSON.stringify({cmd: 'activate', id: +[...document.querySelectorAll('#scroll .tab[data-tab]:not(.active)')].find(n => !n.dataset.pinned).dataset.tab})), 'activated')"
echo 'wait 900'
echo "eval chrome 'glide1 ' + JSON.stringify(glides)"
echo "eval chrome (glides = [], window.__bhipc.postMessage(JSON.stringify({cmd: 'close', id: state.activeTab})), 'closed')"
echo 'wait 900'
echo "eval chrome 'glide2 ' + glides.length + '|' + document.querySelectorAll('.tab-glide, .ghost-out').length"
# Find in page: the page hears none of the keys typed in the bar, a page taking the focus gives it back.
echo "eval content (window.hits = 0, document.addEventListener('keydown', () => hits++, true), addEventListener('keypress', () => hits++), document.body.append(Object.assign(document.createElement('input'), {id: 'q'})), __bhFind(), window.bar = document.activeElement, bar.dispatchEvent(new KeyboardEvent('keydown', {key: 's', bubbles: true, composed: true})), bar.dispatchEvent(new KeyboardEvent('keypress', {key: 's', bubbles: true, composed: true})), document.getElementById('q').focus(), 'find')"
echo 'wait 300'
echo "eval content 'find1 ' + hits + '|' + (document.activeElement === bar)"
echo "eval content (document.getElementById('q').dispatchEvent(new KeyboardEvent('keydown', {key: 's', bubbles: true})), 'find2 ' + hits)"
echo 'quit'
} > /tmp/bh_script.txt
timeout 180 cargo run -q -p blackhole --features devtools >"$work/log" 2>&1 || true
grep -E 'dev\] (eval|state) ->' "$work/log" || true
fail=0
check() { if grep -qF -- "$1" "$work/log"; then echo "ok   $2"; else echo "FAIL $2"; fail=1; fi; }
if grep -qF 'meet no capture' "$work/log"; then
  echo 'skip WebKit screen share hints: no capture on a local http page (Meet itself not covered)'
else
  check 'meet true|setParameters' 'WebKit: screen shares keep their resolution (hints installed)'
fi
check 'pul0 1' 'Pulsar: at the head of the open tabs'
check 'pul1 1|0' 'Pulsar: dragged among the folders'
check 'pul2 0|1' 'Pulsar: dragged back to the open tabs'
check 'home1 true|true' 'start page: the quick access of the space, its hidden section'
check 'home2 true' 'start page: a favorite moved with Alt+Right'
check 'home3 false|false|false' 'second space: its own favorites and sections'
# Three tabs a space with a cap of two: two live in each space (the old global cap left two).
check '"liveTabs":4' 'live tabs: the cap holds per space'
check 'wat0 0|true|false' 'watched areas: the last check shown, nothing being read'
check 'wat1 true|2|watch-shimmer' 'watched areas: "Check all now" reads every one, the rows shimmer'
check 'wat2 false|true|true|true' 'watched areas: new value read, area not found told, checked just now'
check 'wat3 1' 'watched areas: one row read again (its menu), only it shimmers'
check 'glide1 [\"new\",0]' 'active tab: the highlight glides onto the tab chosen'
check 'glide2 0|0' 'active tab closed: its highlight fades with it, no glide'
check 'find1 0|true' 'find in page: keys stay in the bar, the focus comes back to it'
check 'find2 1' 'find in page: the page still hears its own fields'
for s in pulsar-folders home-first home-second watch-checking watch-checked; do
  [[ -f /tmp/bh_shots/v0833-$s.png ]] || { echo "FAIL screenshot v0833-$s missing"; fail=1; }
done
exit $fail
