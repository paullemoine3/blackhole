#!/usr/bin/env bash
# Fingerprinting protection (Shields) on macOS, driven by the devtools harness. No network needed.
# A local page draws a canvas and reads it back: with the protection a few pixels of the readback
# differ from what was drawn, the same way on every read of one site and differently on another
# site; the graphics card is generic, the core count rounded, replaced functions still read as
# native code. Switched off, a page opened afterwards reads exactly what it drew.
# Local server on 127.0.0.1:47989 (also reached as localhost: another site). Scratch profile
# /tmp/bh_data (wiped first).
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
work=$(mktemp -d)
pids=()
cleanup() { kill "${pids[@]}" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT

cat > "$work/http.py" <<'P'
from http.server import BaseHTTPRequestHandler, HTTPServer
PAGE = b"""<!doctype html><title>fp</title><script>
const draw = () => { const c = document.createElement('canvas'); c.width = 200; c.height = 60;
  const x = c.getContext('2d'); x.fillStyle = '#f60'; x.fillRect(0, 0, 200, 60);
  x.fillStyle = '#069'; x.font = '20px Arial'; x.fillText('BlackHole fp', 4, 30); return [c, x]; };
// A hash of what toDataURL returns.
window.__fp = () => { const d = draw()[0].toDataURL(); let h = 0; for (const ch of d) h = (Math.imul(h, 31) + ch.charCodeAt(0)) | 0; return h; };
// How many pixels of a plain orange area read back as something else.
window.__alt = () => { const [, x] = draw(); const d = x.getImageData(0, 0, 200, 8).data; let n = 0;
  for (let i = 0; i < d.length; i += 4) if (d[i] !== 255 || d[i + 1] !== 102 || d[i + 2] !== 0) n++; return n; };
window.__gl = () => { const g = document.createElement('canvas').getContext('webgl'); if (!g) return 'nogl';
  const e = g.getExtension('WEBGL_debug_renderer_info'); return e ? g.getParameter(e.UNMASKED_VENDOR_WEBGL) + '|' + g.getParameter(e.UNMASKED_RENDERER_WEBGL) : 'noext'; };
</script>"""
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.send_header("Content-Length", str(len(PAGE)))
        self.end_headers()
        self.wfile.write(PAGE)
    def log_message(self, *a):
        pass
HTTPServer(("127.0.0.1", 47989), H).serve_forever()
P
python3 -I "$work/http.py" & pids+=($!)
sleep 1

cat > /tmp/bh_script.txt <<'S'
wait 2000
cmd {"cmd":"set_language","language":"en"}
wait 300
cmd {"cmd":"open","input":"http://127.0.0.1:47989/a"}
wait 3000
eval content 'alt ' + __alt()
eval content 'a1 ' + __fp()
eval content 'a2 ' + __fp()
eval content 'gl ' + __gl()
eval content 'cores ' + navigator.hardwareConcurrency
eval content 'native ' + /native code/.test(HTMLCanvasElement.prototype.toDataURL.toString()) + ' ' + HTMLCanvasElement.prototype.toDataURL.name
eval content 'battery ' + ('getBattery' in navigator)
cmd {"cmd":"open","input":"http://localhost:47989/b"}
wait 3000
eval content 'b1 ' + __fp()
cmd {"cmd":"set_shield","name":"fingerprinting","on":false}
wait 1500
cmd {"cmd":"open","input":"http://127.0.0.1:47989/c"}
wait 3000
eval content 'off ' + __alt()
eval chrome 'alive'
quit
S
out=$(timeout 180 cargo run -q -p blackhole --features devtools 2>&1 | tee /tmp/bh_fingerprint.log | grep "dev\] eval ->" || true)
mapfile -t L <<<"$out"
fail=0
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
value() { sed -E 's/.*"[a-z0-9]+ (-?[0-9]+)".*/\1/' <<<"${L[$1]:-}"; }
alt=$(value 0)
if [[ "$alt" =~ ^[0-9]+$ ]] && (( alt > 0 && alt < 100 )); then echo "ok   canvas readback varies by a few pixels ($alt)"; else echo "FAIL canvas readback varies (got: ${L[0]:-none})"; fail=1; fi
if [[ -n "$(value 1)" && "$(value 1)" == "$(value 2)" ]]; then echo "ok   the same on every read of one site"; else echo "FAIL stable on one site (got: ${L[1]:-} / ${L[2]:-})"; fail=1; fi
if [[ "${L[3]:-}" == *'WebKit|WebKit WebGL'* || "${L[3]:-}" == *'noext'* || "${L[3]:-}" == *'nogl'* ]]; then echo "ok   graphics card generic"; else echo "FAIL graphics card generic (got: ${L[3]:-none})"; fail=1; fi
check 4 'cores ' "core count read"
[[ "${L[4]:-}" =~ cores\ (2|4|8)\" ]] && echo "ok   core count rounded" || { echo "FAIL core count rounded (got: ${L[4]:-none})"; fail=1; }
check 5 'native true toDataURL' "replaced functions read as native, same name"
check 6 'battery false' "no battery API"
if [[ -n "$(value 7)" && "$(value 7)" != "$(value 1)" ]]; then echo "ok   another site reads other values"; else echo "FAIL another site differs (got: ${L[7]:-} vs ${L[1]:-})"; fail=1; fi
check 8 'off 0"' "switched off: a new page reads what it drew"
check 9 'alive' "no crash: the app is still running"
exit $fail
