#!/usr/bin/env bash
# Picture in Picture on macOS, driven by the devtools harness: a video floats when its tab is
# left, the sidebar card's buttons (play/pause, sound, PiP, close) work, coming back to the tab
# brings it home, and the floating window's buttons are told apart: "back to tab" (the video
# keeps playing) shows its tab, close (it pauses) switches nothing. The harness cannot click
# the native window, so the page plays both: leaving PiP playing, or paused. A page posting
# "pip_left" for a video the shell never floated switches nothing.
# A call's tiles are muted live streams: WebKit pauses one that comes back into a hidden page,
# which must still read as "back to tab" (call.html, a canvas stream).
# A CC0 clip from MDN is served on 127.0.0.1:47995 (needs network access once per run).
# Scratch profile /tmp/bh_data (wiped first); screenshots in /tmp/bh_shots/pip-*.png.
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
site=$(mktemp -d)
srv=
cleanup() { [[ -n "$srv" ]] && kill "$srv" 2>/dev/null || true; rm -rf "$site"; }
trap cleanup EXIT
curl -sfL -o "$site/flower.mp4" https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4
cat > "$site/video.html" <<'H'
<!doctype html><title>bh video</title><body style="margin:0;background:#000"><video src="flower.mp4" loop playsinline style="width:100%"></video></body>
H
cat > "$site/call.html" <<'H'
<!doctype html><title>bh call</title><body style="margin:0;background:#000"><canvas width="640" height="360" hidden></canvas><video muted playsinline style="width:100%"></video>
<script>
const c = document.querySelector('canvas'), x = c.getContext('2d'); let n = 0;
setInterval(() => { x.fillStyle = 'hsl(' + (n++ % 360) + ',60%,50%)'; x.fillRect(0, 0, 640, 360); }, 50);
document.querySelector('video').srcObject = c.captureStream(20);
</script></body>
H
cat > "$site/other.html" <<'H'
<!doctype html><title>bh other</title><p>other</p>
H
python3 -I -m http.server 47995 --bind 127.0.0.1 --directory "$site" >/dev/null 2>&1 & srv=$!
sleep 1

# Tab 2 plays the video, tab 3 is another page (ids of a fresh profile).
V="document.querySelector('video')"
STATE="(\$V.webkitPresentationMode === 'picture-in-picture' ? 'pip' : 'inline') + (\$V.paused ? ' paused' : ' playing')"
STATE=${STATE//\$V/$V}
ACTIVE="eval chrome 'active=' + state.activeTab"
CARD="eval chrome 'card=' + !!document.querySelector('#media .mc')"
cat > /tmp/bh_script.txt <<S
wait 2000
cmd {"cmd":"open","input":"http://127.0.0.1:47995/video.html"}
wait 4000
eval content (() => { const v = $V; v.muted = false; v.play(); return 'play'; })()
wait 2500
cmd {"cmd":"open","input":"http://127.0.0.1:47995/other.html"}
wait 4000
eval tab2 'auto ' + $STATE
$CARD
shot pip-auto
cmd {"cmd":"media_ctl","id":2,"action":"pip"}
wait 2000
eval tab2 'card-pip ' + $STATE
$ACTIVE
cmd {"cmd":"media_ctl","id":2,"action":"pip"}
wait 2000
eval tab2 'card-pip-again ' + $STATE
cmd {"cmd":"media_ctl","id":2,"action":"toggle"}
wait 1500
eval tab2 'card-pause ' + $STATE
cmd {"cmd":"media_ctl","id":2,"action":"toggle"}
wait 1500
eval tab2 'card-play ' + $STATE
cmd {"cmd":"media_ctl","id":2,"action":"mute"}
wait 1500
eval tab2 'muted=' + $V.muted
cmd {"cmd":"media_ctl","id":2,"action":"mute"}
wait 2500
eval tab2 (() => { const v = $V; v.pause(); v.webkitSetPresentationMode('inline'); return 'window-close'; })()
wait 2000
eval tab2 'after-close ' + $STATE
$ACTIVE
eval tab2 (() => { $V.play(); return 'replay'; })()
wait 2000
cmd {"cmd":"media_ctl","id":2,"action":"pip"}
wait 2500
eval tab2 (() => { $V.webkitSetPresentationMode('inline'); return 'window-back'; })()
wait 2500
$ACTIVE
eval tab2 'after-back ' + $STATE
shot pip-back
cmd {"cmd":"open","input":"http://127.0.0.1:47995/other.html"}
wait 4000
eval tab2 'auto-again ' + $STATE
cmd {"cmd":"activate","id":2}
wait 2500
eval tab2 'home ' + $STATE
$ACTIVE
cmd {"cmd":"activate","id":3}
wait 3000
eval tab3 (() => { window.__bhipc.postMessage(JSON.stringify({ cmd: 'pip_left', paused: false })); return 'forged'; })()
wait 1500
$ACTIVE
cmd {"cmd":"media_ctl","id":2,"action":"close"}
wait 2000
eval tab2 'card-close ' + $STATE
$CARD
cmd {"cmd":"open","input":"http://127.0.0.1:47995/call.html"}
wait 4000
eval content (() => { $V.play(); return 'live-play'; })()
wait 1500
cmd {"cmd":"activate","id":3}
wait 4000
eval tab4 'live-auto ' + $STATE
eval tab4 (() => { $V.webkitSetPresentationMode('inline'); return 'live-back'; })()
wait 3000
$ACTIVE
eval tab4 'live-after ' + $STATE
cmd {"cmd":"activate","id":3}
wait 4000
eval tab4 (() => { const v = $V; v.pause(); v.webkitSetPresentationMode('inline'); return 'live-close'; })()
wait 3000
$ACTIVE
# WebKit switches the video back to inline when "back to tab" is pressed but tells the page only
# once the window has shrunk back: the shell's probe sees it first. The page's event is held
# back 1.5 s here; it must still show the tab.
eval tab2 (() => { $V.play(); return 'late-replay'; })()
wait 1500
cmd {"cmd":"activate","id":2}
wait 2500
cmd {"cmd":"activate","id":3}
wait 4000
eval tab2 'late-auto ' + $STATE
eval tab2 (() => { const v = $V; const hold = (e) => { if (e.target === v) e.stopImmediatePropagation(); }; addEventListener('webkitpresentationmodechanged', hold, true); v.webkitSetPresentationMode('inline'); setTimeout(() => { removeEventListener('webkitpresentationmodechanged', hold, true); window.__bhipc.postMessage(JSON.stringify({ cmd: 'pip_left', paused: false })); }, 1500); return 'late-back'; })()
wait 4000
$ACTIVE
eval chrome 'alive'
quit
S
out=$(timeout 300 cargo run -q -p blackhole --features devtools 2>&1 | grep "dev\] eval ->" || true)
mapfile -t L <<<"$out"
fail=0
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
check 0 'play' "the video plays in its tab"
check 1 'auto pip playing' "leaving the tab floats the video"
check 2 'card=true' "the sidebar shows its player card"
check 3 'card-pip inline playing' "card PiP button: back into the page"
check 4 'active=3' "card PiP button: no tab switch"
check 5 'card-pip-again pip playing' "card PiP button: floats it again"
check 6 'card-pause pip paused' "card play/pause: pauses"
check 7 'card-play pip playing' "card play/pause: plays again"
check 8 'muted=true' "card sound button: mutes"
check 9 'window-close' "floating window: close (pauses, leaves)"
check 10 'after-close inline paused' "close: the video stays paused in its page"
check 11 'active=3' "close: no tab switch"
check 12 'replay' "the video plays again"
check 13 'window-back' "floating window: back to tab (leaves, still playing)"
check 14 'active=2' "back to tab: its tab is shown"
check 15 'after-back inline playing' "back to tab: the video goes on in its page"
check 16 'auto-again pip playing' "leaving the tab again floats it again"
check 17 'home inline playing' "coming back to the tab brings it home"
check 18 'active=2' "coming back to the tab: no extra switch"
check 19 'forged' "a page posts a pip_left of its own"
check 20 'active=3' "a page cannot switch tabs with it"
check 21 'card-close inline paused' "card close: pauses and closes the floating window"
check 22 'card=false' "card close: the card goes away"
check 23 'live-play' "a muted live stream (a call's tile) plays"
check 24 'live-auto pip playing' "live stream: leaving the tab floats it"
check 25 'live-back' "live stream: back to tab"
check 26 'active=4' "live stream: back to tab shows its tab"
check 27 'live-after inline playing' "live stream: it goes on in its page"
check 28 'live-close' "live stream: close (pauses, leaves)"
check 29 'active=3' "live stream: close switches no tab"
check 30 'late-replay' "the first video plays again"
check 31 'late-auto pip playing' "leaving its tab floats it"
check 32 'late-back' "back to tab, the page told late (WebKit's animation)"
check 33 'active=2' "back to tab told late: its tab is shown"
check 34 'alive' "no crash: the app is still running"
for s in pip-auto pip-back; do
  [[ -f /tmp/bh_shots/$s.png ]] || { echo "FAIL screenshot $s missing"; fail=1; }
done
exit $fail
