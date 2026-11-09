#!/usr/bin/env bash
# Screenshots of the README (macOS), driven by the devtools harness on a scratch profile
# (/tmp/bh_data, wiped first): sidebar, start page, what's new, Settings, an error page and an
# Easel with a live area. Written to docs/screenshots/ at 1600 px wide. Needs network access;
# keep the BlackHole window visible and in front while it runs (captures are taken on screen).
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
run() { timeout 240 cargo run -q -p blackhole --features devtools >/dev/null 2>&1 || true; }

# 1. A lived-in profile: tabs, a folder, a meeting going on, an error page, Settings, an Easel.
cat > /tmp/bh_script.txt <<'S'
wait 2500
cmd {"cmd":"open","input":"https://www.wikipedia.org"}
wait 3000
cmd {"cmd":"open","input":"https://news.ycombinator.com"}
wait 3000
cmd {"cmd":"open","input":"https://developer.mozilla.org/en-US/"}
wait 3000
cmd {"cmd":"open","input":"https://www.rust-lang.org"}
wait 4000
cmd {"cmd":"new_folder"}
wait 600
eval chrome (() => { const i = document.querySelector('.fedit'); if (i) i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })); send({ cmd: 'rename_folder', id: state.folders[0].id, name: 'Research' }); return 'x'; })()
wait 600
meeting https://meet.google.com/abc-defg-hij Weekly sync
wait 2000
shot sidebar
cmd {"cmd":"home"}
wait 2000
shot home
cmd {"cmd":"open","input":"http://127.0.0.1:47999/"}
wait 5000
shot errors
cmd {"cmd":"open_settings","tab":"privacy"}
wait 2000
shots settings
cmd {"cmd":"close_settings"}
wait 800
cmd {"cmd":"open","input":"https://example.com"}
wait 4000
cmd {"cmd":"new_easel","name":"Research"}
cmd {"cmd":"open_easel"}
wait 2000
cmd {"cmd":"capture_to_easel"}
wait 1500
cmd {"cmd":"open","input":"https://example.com"}
wait 4000
cmd {"cmd":"live_pick"}
wait 500
eval content (() => { const h = document.querySelector('p').getBoundingClientRect(); const x = h.left + h.width / 2, y = h.top + h.height / 2; for (const t of ['mousemove', 'click']) document.elementFromPoint(x, y).dispatchEvent(new MouseEvent(t, { bubbles: true, cancelable: true, clientX: x, clientY: y, view: window })); return 'picked'; })()
wait 1000
cmd {"cmd":"live_capture_to","easel":1}
wait 1500
cmd {"cmd":"easel_add_note","x":40,"y":240}
wait 800
eval easel (() => { const a = [...document.querySelectorAll('.card textarea')].pop(); if (a) { a.value = 'Compare with the docs before Friday'; a.dispatchEvent(new Event('input', { bubbles: true })); a.blur(); } return 'x'; })()
cmd {"cmd":"easel_live_play","item":2}
wait 7000
shote easel
quit
S
run

# 2. Same profile, as if just updated from 0.8.0: the start page shows what's new.
python3 -I - <<'P'
import json, pathlib
p = next(pathlib.Path("/tmp/bh_data").rglob("settings.json"))
s = json.loads(p.read_text())
s["last_seen_version"] = "0.8.0"
p.write_text(json.dumps(s))
P
cat > /tmp/bh_script.txt <<'S'
wait 3000
cmd {"cmd":"home"}
wait 2500
shot whatsnew
quit
S
run

mkdir -p docs/screenshots
for s in sidebar home whatsnew settings errors easel; do
  if [[ -f /tmp/bh_shots/$s.png ]]; then
    sips -Z 1600 "/tmp/bh_shots/$s.png" --out "docs/screenshots/$s.png" >/dev/null
    echo "ok   docs/screenshots/$s.png"
  else
    echo "FAIL $s.png missing"
  fi
done
