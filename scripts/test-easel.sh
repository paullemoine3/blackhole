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
wait 8000
eval easel document.querySelector('.card.live .play').textContent
eval live (() => { const r = document.querySelector('p').getBoundingClientRect(); return 'crop ' + (Math.abs(r.left) < 2 && Math.abs(r.top) < 2 && r.width <= innerWidth + 3 && r.height <= innerHeight + 3); })()
shote 03-easel-live
cmd {"cmd":"easel_live_stop","item":2}
wait 500
eval easel document.querySelector('.card.live .play').textContent
cmd {"cmd":"new_folder"}
wait 600
eval chrome (() => { const i = document.querySelector('.fedit'); if (i) i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' })); return 'x'; })()
wait 600
eval chrome (() => { send({ cmd: 'move_easel', id: state.easels[0].id, folder: state.folders[0].id }); return 'dragged'; })()
wait 1000
eval chrome JSON.stringify({ loose: 0, inFolder: document.querySelectorAll('.fb .tab.easel').length })
eval chrome (() => { document.querySelector('.fb .tab.easel').click(); return 'clicked'; })()
wait 1500
eval etab JSON.stringify({ embedded: document.body.classList.contains('embedded'), nav: getComputedStyle(document.querySelector('nav')).display, cards: document.querySelectorAll('.card').length, name: document.getElementById('title').value })
eval chrome JSON.stringify({ home: state.home, open: state.easels.map(e => e.open) })
cmd {"cmd":"easel_live_play","item":2}
wait 8000
eval live (() => { const r = document.querySelector('p').getBoundingClientRect(); return 'tabcrop ' + (Math.abs(r.left) < 2 && Math.abs(r.top) < 2 && r.width <= innerWidth + 3 && r.height <= innerHeight + 3 && innerWidth > 50); })()
shot 04-easel-tab
cmd {"cmd":"close_easel_tab"}
wait 800
eval chrome JSON.stringify({ home: state.home, open: state.easels.map(e => e.open) })
eval chrome JSON.stringify({ before: document.getElementById('folders').previousElementSibling.className, bms: !!document.getElementById('bms'), bmhead: !!document.getElementById('bmhead') })
cmd {"cmd":"import_html","text":"<DL><DT><H3>ImpFolder</H3><DL><DT><A HREF=\"https://imp1.test/\">Imp One</A><DT><A HREF=\"https://imp2.test/\">Imp Two</A></DL></DL>"}
wait 800
eval chrome (() => { const f = state.folders.find(f => f.name === 'ImpFolder'); const t = f ? state.tabs.filter(t => t.folder === f.id).map(t => t.title) : []; return 'imp ' + JSON.stringify(t) + ' bms ' + document.querySelectorAll('#folders .fh').length; })()
cmd {"cmd":"clear_tabs"}
wait 600
cmd {"cmd":"open_library"}
wait 1000
eval library JSON.stringify({ rail: document.querySelectorAll('.nav').length, archived: S.archive.length > 0, sp: S.spaces.length })
shot 05-library
cmd {"cmd":"close_library"}
wait 500
eval chrome JSON.stringify({ home: state.home })
eval chrome (() => { const t = state.tabs[0]; if (!t) return 'notab'; window.bh.render({ ...state, activeTab: null, media: [{ id: t.id, playing: true, muted: false }] }); const card = document.querySelector('#media .mc'); const btns = card ? [...card.querySelectorAll('button')].map(b => b.textContent) : []; const snd = !!document.querySelector('.tab .snd'); window.bh.render({ ...state, activeTab: t.id, media: [{ id: t.id, playing: true, muted: false }] }); return 'media ' + JSON.stringify({ card: !!card, btns, snd, hiddenWhenActive: !document.querySelector('#media .mc') }); })()
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
check 16 'x' "folder: created"
check 17 'dragged' "sidebar: easel dragged onto a folder"
check 18 'inFolder\":1' "sidebar: easel now lives in the folder"
check 19 'clicked' "tab: easel opened from the sidebar"
check 20 'embedded\":true' "tab: easel page is embedded in the main window"
check 20 'nav\":\"none' "tab: the list of easels is hidden"
check 20 'name\":\"Renamed' "tab: shows the clicked easel"
check 21 'open\":[true]' "tab: sidebar marks the easel open"
check 22 'tabcrop true' "tab: live card plays cropped inside the content area"
check 23 'open\":[false]' "tab: closing returns to the page"
check 24 'before\":\"section secrow\"' "pinned: header sits right above the folders"
check 24 'bms\":false' "imports: no separate bookmark list, they are folders and tabs"
check 25 'Imp One' "import: bookmarks become tabs filed in a real folder"
check 25 'Imp Two' "import: every link is a regular tab"
check 26 'rail\":5' "library: five sections in the rail"
check 26 'archived\":true' "library: cleared tab is archived"
check 27 'home' "library: closes back to the browser"
check 28 'card\":true' "media: a card appears for a tab playing in the background"
check 28 '⏸' "media: the card offers pause"
check 28 '⧉' "media: the card offers Picture in Picture"
check 28 'snd\":true' "media: the playing tab shows a speaker icon"
check 28 'hiddenWhenActive\":true' "media: no card for the tab being viewed"
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
