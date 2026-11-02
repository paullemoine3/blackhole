#!/usr/bin/env bash
# End-to-end check of Easels on macOS, driven by the devtools harness: create, capture a live
# page, notes, edits, link back to the page, rename, delete, live capture (pick an area, play it,
# check the crop), then persistence on disk.
# Uses the scratch profile /tmp/bh_data (wiped first); screenshots land in /tmp/bh_shots. Needs network access.
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
cat > /tmp/bh_script.txt <<'S'
wait 2000
cmd {"cmd":"open","input":"https://example.com"}
wait 5000
cmd {"cmd":"new_easel","name":"Ideas"}
cmd {"cmd":"open_easel"}
wait 2000
eval easel document.getElementById('title').value
cmd {"cmd":"capture_to_easel"}
wait 1500
eval easel JSON.stringify([...document.querySelectorAll('.card.clip .link')].map(a => a.textContent))
eval easel document.querySelector('.card.clip .host').textContent
cmd {"cmd":"easel_add_note"}
wait 500
eval easel document.querySelectorAll('.card').length
eval easel (() => { const t = document.querySelector('.card.note textarea'); t.value = 'hello easel'; t.dispatchEvent(new Event('input')); return 'typed'; })()
wait 600
cmd {"cmd":"easel_move","item":2,"x":420,"y":300}
cmd {"cmd":"easel_color","item":2,"color":"#2e8b6e"}
wait 500
shote 01-easel-board
cmd {"cmd":"home"}
wait 800
cmd {"cmd":"capture_to_easel"}
wait 800
eval easel document.querySelectorAll('.card').length
cmd {"cmd":"open","input":"https://example.org"}
wait 4000
cmd {"cmd":"easel_open_link","item":1}
wait 4000
eval content location.href
cmd {"cmd":"rename_easel","id":1,"name":"Renamed"}
cmd {"cmd":"new_easel","name":"Second"}
wait 500
eval easel JSON.stringify([...document.querySelectorAll('.easel .name')].map(n => n.textContent))
shote 02-easel-two-boards
cmd {"cmd":"delete_easel","id":2}
wait 500
eval easel document.querySelectorAll('.easel').length
cmd {"cmd":"easel_remove","item":2}
wait 500
eval easel document.querySelectorAll('.card').length
cmd {"cmd":"open","input":"https://example.com"}
wait 4000
cmd {"cmd":"live_pick"}
wait 500
eval content String(window.__bhPick === true)
eval content (() => { const h = document.querySelector('p').getBoundingClientRect(); const x = h.left + h.width / 2, y = h.top + h.height / 2; for (const t of ['mousemove', 'click']) document.elementFromPoint(x, y).dispatchEvent(new MouseEvent(t, { bubbles: true, cancelable: true, clientX: x, clientY: y, view: window })); return 'picked'; })()
wait 1500
eval easel JSON.stringify([...document.querySelectorAll('.card.live .slot')].map(c => c.textContent))
cmd {"cmd":"easel_live_play","item":2}
wait 5000
eval easel document.querySelector('.card.live .play').textContent
eval live (() => { const r = document.querySelector('p').getBoundingClientRect(); return 'crop ' + (Math.abs(r.left) < 2 && Math.abs(r.top) < 2 && r.width <= innerWidth + 3 && r.height <= innerHeight + 3); })()
shote 03-easel-live
cmd {"cmd":"easel_live_stop","item":2}
wait 500
eval easel document.querySelector('.card.live .play').textContent
quit
S
out=$(timeout 180 cargo run -q -p blackhole --features devtools 2>&1 | grep 'dev\] eval ->' || true)
mapfile -t L <<<"$out"
fail=0
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
check 0 'Ideas' "easel: created and shown"
check 1 'Example Domain' "capture: clip carries the page title"
check 2 'example.com' "capture: clip shows its source host"
check 3 '2' "note: added next to the clip"
check 4 'typed' "note: text typed"
check 5 '2' "capture: home page is refused (still 2 cards)"
check 6 'example.com' "clip link: opens the captured page in a tab"
check 7 'Renamed' "rename: easel list updated"
check 7 'Second' "new easel: listed"
check 8 '1' "delete: only one easel left"
check 9 '1' "remove: note deleted, one card left"
check 10 'true' "live: picker armed on the page"
check 11 'picked' "live: area clicked"
check 12 'Paused' "live: card added, paused until played"
check 13 '⏸' "live: playing"
check 14 'crop true' "live: view shows the picked area, scaled to fill the card"
check 15 '▶' "live: paused again"
[[ -f /tmp/bh_shots/01-easel-board.png && -f /tmp/bh_shots/03-easel-live.png ]] || { echo "FAIL screenshot missing"; fail=1; }
python3 - <<'P' || fail=1
import json, sys
d = json.load(open('/tmp/bh_data/easels.json'))
ok = len(d) == 1 and d[0]['name'] == 'Renamed' and len(d[0]['items']) == 2
clip, live = d[0]['items'] if ok else ({}, {})
ok = ok and clip.get('kind') == 'clip' and clip.get('url', '').startswith('https://example.com')
ok = ok and live.get('kind') == 'live' and 'p:nth-of-type' in live.get('selector', '') and live.get('url', '').startswith('https://example.com')
print('ok   persistence: easels.json holds the renamed easel, its clip and the live area' if ok else 'FAIL persistence: %r' % d)
sys.exit(0 if ok else 1)
P
exit $fail
