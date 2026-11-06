#!/usr/bin/env bash
# Checks the "Site injoignable" error page on macOS: a refused connection (closed local port)
# shows the themed page, keeps the URL in the address bar and offers a retry. No network needed.
# Scratch profile /tmp/bh_data (wiped first); screenshot in /tmp/bh_shots/unreachable.png.
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
cat > /tmp/bh_script.txt <<'S'
wait 2000
cmd {"cmd":"open","input":"https://nonexistent.invalid/"}
wait 28000
eval content document.querySelector('h1') ? document.querySelector('h1').textContent : 'none'
eval content document.getElementById('url') ? document.getElementById('url').textContent : 'none'
eval content typeof window.__bhRetry
shot unreachable
quit
S
out=$(timeout 180 cargo run -q -p blackhole --features devtools 2>&1 | grep "dev\] eval ->" || true)
mapfile -t L <<<"$out"
fail=0
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
check 0 'Site injoignable' "error page: title shown"
check 1 'nonexistent.invalid' "error page: failed URL shown"
check 2 'function' "error page: retry available"
[[ -f /tmp/bh_shots/unreachable.png ]] || { echo "FAIL screenshot missing"; fail=1; }
exit $fail
