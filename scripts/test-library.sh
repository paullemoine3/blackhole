#!/usr/bin/env bash
# The Library and the sidebar's cards on macOS, driven by the devtools harness: a local download
# finishes (the tab opened for it closes), its sidebar card says done, and a click on it opens
# the Library on Downloads; then
# every section renders on the design system (rail counts, filter, search, empty states) and
# the "animations off" setting is honoured. No network needed.
# The app runs with a scratch HOME so the downloaded file lands in a throwaway Downloads folder;
# profile /tmp/bh_data (wiped first); screenshots in /tmp/bh_shots/library-*.png.
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
        body = b"BlackHole test download\n" * 2000
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Disposition", 'attachment; filename="bh-report.bin"')
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a):
        pass
ThreadingHTTPServer(("127.0.0.1", 47994), H).serve_forever()
P
python3 -I "$work/srv.py" & srv=$!
cargo build -q -p blackhole --features devtools
sleep 1

cat > /tmp/bh_script.txt <<'S'
wait 2000
cmd {"cmd":"set_language","language":"en"}
cmd {"cmd":"open","input":"https://example.test/"}
wait 1500
cmd {"cmd":"open","input":"http://127.0.0.1:47994/report"}
wait 2500
eval chrome 'card:' + (() => { const c = document.querySelector('#dls .dl'); return c ? c.className + '|' + c.querySelector('.pc').textContent : 'none'; })()
eval chrome (() => { document.querySelector('#dls .dl').click(); return 'clicked'; })()
wait 2000
eval library 'sec:' + document.querySelector('.nav.on').dataset.s + '|' + document.querySelector('h1').textContent
eval library 'rows:' + document.querySelectorAll('main .card .row').length + '|' + document.querySelector('main .row b').textContent + '|' + document.querySelector('main .row .ico').className
eval library 'rail:' + document.querySelector('.count[data-c="downloads"]').textContent + '|' + document.querySelectorAll('nav svg.bh-i').length
shot library-downloads
eval library (() => { [...document.querySelectorAll('.seg button')].find(b => b.getAttribute('aria-checked') === 'false' && b.textContent.toLowerCase().includes('fail')).click(); return 'failed'; })()
wait 400
eval library 'flt:' + !!document.querySelector('main .empty .halo svg') + '|' + document.querySelector('.seg button.on').textContent
eval library (() => { document.querySelector('.seg button').click(); const f = document.getElementById('q'); f.focus(); f.value = 'zzz'; f.dispatchEvent(new Event('input', { bubbles: true })); return 'search'; })()
wait 300
eval library 'q:' + document.querySelectorAll('main .row').length + '|' + (document.activeElement && document.activeElement.id)
eval library (() => { document.querySelector('.nav[data-s="spaces"]').click(); return 'spaces'; })()
wait 900
eval library 'spaces:' + document.querySelector('h1').textContent + '|' + document.querySelectorAll('.col').length + '|' + document.querySelectorAll('.col.on').length
eval library (() => { document.querySelector('.nav[data-s="history"]').click(); return 'history'; })()
wait 900
eval library 'hist:' + document.querySelectorAll('.day').length + '|' + document.querySelectorAll('main .row').length
shot library-history
eval library (() => { document.querySelector('.nav[data-s="watches"]').click(); return 'watches'; })()
wait 900
eval library 'watch:' + !!document.querySelector('.empty .halo svg') + '|' + document.querySelector('.empty p').textContent.length
eval library 'html:' + document.querySelectorAll('main [onclick]').length
cmd {"cmd":"set_animations","on":false}
wait 600
eval library 'noanim:' + document.body.classList.contains('noanim')
cmd {"cmd":"set_animations","on":true}
cmd {"cmd":"close_library"}
wait 600
eval chrome 'dltab:' + state.tabs.filter(t => (t.url || '').includes('47994')).length + '|' + document.body.innerText.includes('unreachable')
eval chrome 'alive'
quit
S
out=$(HOME="$work/home" timeout 180 target/debug/blackhole 2>&1 | grep "dev\] eval ->" || true)
mapfile -t L <<<"$out"
fail=0
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
check 0 'card:dl ok|Done' "sidebar: the finished download's card says Done"
check 1 'clicked' "sidebar: the card can be clicked"
check 2 'sec:downloads|Downloads' "the click opens the Library on Downloads"
check 3 'rows:1|bh-report.bin|ico ok' "the download is listed, marked done"
check 4 'rail:1|' "rail: count and drawn icons"
check 5 'failed' "filter: pick Failed"
check 6 'flt:true|' "filter: an empty state with its icon"
check 7 'search' "search: type in the field"
check 8 'q:0|q' "search: no match, the field keeps the focus"
check 9 'spaces' "rail: go to Spaces"
check 10 'spaces:Spaces|1|1' "spaces: one column, the active one lit"
check 11 'history' "rail: go to History"
check 12 'hist:1|' "history: grouped by day"
check 13 'watches' "rail: go to Watched Areas"
check 14 'watch:true|' "watched areas: empty state with its icon"
check 15 'html:0' "rows are built with handlers, no inline markup"
check 16 'noanim:true' "animations off reaches the Library"
check 17 'dltab:0|false' "the tab opened for the file closes (never \"Site unreachable\")"
check 18 'alive' "no crash: the app is still running"
ls "$work/home/Downloads" | grep -q bh-report || { echo "FAIL the file did not land in the scratch Downloads"; fail=1; }
for s in library-downloads library-history; do
  [[ -f /tmp/bh_shots/$s.png ]] || { echo "FAIL screenshot $s missing"; fail=1; }
done
exit $fail
