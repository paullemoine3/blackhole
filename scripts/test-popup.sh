#!/usr/bin/env bash
# Sign-in popups and Trusted Types on macOS, driven by the devtools harness. No network needed.
# 1. A page opens a sized popup (`window.open(url, name, "width=…")`, the OAuth/GSI pattern): the
#    popup must keep `window.opener` (it posts a message back to the page) and close itself with
#    `window.close()`. Scripts run by the harness count as a user gesture in WKWebView.
# 2. A page under `require-trusted-types-for 'script'` (as accounts.google.com): BlackHole's
#    injected scripts (autofill, find bar) must not assign HTML strings (no TrustedHTML violation).
# Local server on 127.0.0.1:47990. Scratch profile /tmp/bh_data (wiped first).
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
work=$(mktemp -d)
pids=()
cleanup() { kill "${pids[@]}" 2>/dev/null || true; rm -rf "$work"; }
trap cleanup EXIT

cat > "$work/http.py" <<'P'
from http.server import BaseHTTPRequestHandler, HTTPServer
PAGES = {
    "/opener": (b"""<!doctype html><title>opener</title><p>opener</p><script>
window.addEventListener('message', (e) => { document.title = 'got ' + e.data; });
</script>""", None),
    "/popup": (b"""<!doctype html><title>popup</title><p>popup</p><script>
window.opener ? window.opener.postMessage('hello', '*') : (document.title = 'no opener');
setTimeout(() => window.close(), 800);
</script>""", None),
    "/tt": (b"""<!doctype html><title>tt</title><script>
window.__violations = [];
document.addEventListener('securitypolicyviolation', (e) => window.__violations.push(e.violatedDirective));
</script><form><input type=email name=u><input type=password name=p></form>""",
            "require-trusted-types-for 'script'"),
}
class H(BaseHTTPRequestHandler):
    def do_GET(self):
        body, csp = PAGES.get(self.path, (b"<!doctype html><title>bh ok</title>", None))
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        if csp:
            self.send_header("Content-Security-Policy", csp)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a):
        pass
HTTPServer(("127.0.0.1", 47990), H).serve_forever()
P
python3 -I "$work/http.py" & pids+=($!)
sleep 1

cat > /tmp/bh_script.txt <<'S'
wait 2000
cmd {"cmd":"open","input":"http://127.0.0.1:47990/opener"}
wait 3000
eval content (window.__w = window.open('/popup', 'bhpopup', 'width=420,height=360'), window.__w ? 'opened' : 'blocked')
wait 4000
eval content document.title
eval content 'closed ' + (window.__w && window.__w.closed)
cmd {"cmd":"open","input":"http://127.0.0.1:47990/tt"}
wait 3000
eval content 'tt ' + typeof window.trustedTypes
eval content 'autofill ' + typeof window.__bhAutofill
eval content (() => { try { window.__bhFind('open'); return 'find ok'; } catch (e) { return 'find ' + e; } })()
wait 500
eval content 'violations ' + JSON.stringify(window.__violations)
eval chrome 'alive'
quit
S
out=$(timeout 180 cargo run -q -p blackhole --features devtools 2>&1 | tee /tmp/bh_popup.log | grep "dev\] eval ->" || true)
mapfile -t L <<<"$out"
fail=0
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
check 0 'opened' "popup: window.open with features opens a window"
check 1 'got hello' "popup: keeps window.opener (message posted back)"
check 2 'closed true' "popup: window.close() closes it"
check 3 'tt ' "trusted types: support reported"
check 4 'autofill object' "trusted types: autofill.js runs to the end"
check 5 'find ok' "trusted types: find bar opens"
check 6 'violations []' "trusted types: no violation from injected scripts"
check 7 'alive' "no crash: the app is still running"
exit $fail
