#!/usr/bin/env bash
# End-to-end check of the extension catalog on macOS, driven by the devtools harness.
# Uses the scratch profile /tmp/bh_data (wiped first). Needs network access.
# The built-in blockers (trackers and the ads shield) are turned off first: they block the same
# ad hosts, so they would hide whether the extension is on or off. The extension's rules take a
# variable time to apply, so each probe retries (up to ~15 s) until it sees the expected state.
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
ID=ublock-origin-lite
# probe true|false: fetches the three URLs until the two trackers are blocked (true) or the ad
# host answers (false), at most 10 tries; the last result lands in window.__r.
probe() { echo "(async()=>{const W=$1;let o;for(let i=0;i<10;i++){o={};for(const u of ['https://example.org/','https://c.amazon-adsystem.com/aax2/apstag.js','https://js-agent.newrelic.com/nr-loader-spa-current.min.js']){try{await fetch(u,{mode:'no-cors',cache:'no-store'});o[u]='ok'}catch(e){o[u]='NETERR'}};const v=Object.values(o);if(W?(v[1]==='NETERR'&&v[2]==='NETERR'):v[1]==='ok')break;await new Promise(r=>setTimeout(r,1500))};window.__r=JSON.stringify(o)})();'go'"; }
STATUS="eval settings JSON.stringify(document.querySelector('#catalog .sub').textContent)"
cat > /tmp/bh_script.txt <<S
wait 2000
cmd {"cmd":"set_language","language":"en"}
cmd {"cmd":"set_tracker_blocking","on":false}
cmd {"cmd":"set_shield","name":"ads","on":false}
cmd {"cmd":"open_settings","tab":"extensions"}
wait 1500
$STATUS
cmd {"cmd":"install_catalog","id":"$ID"}
wait 9000
$STATUS
cmd {"cmd":"open","input":"https://example.com"}
wait 4000
eval content $(probe true)
wait 18000
eval content window.__r
cmd {"cmd":"set_extension_enabled","id":"$ID","enabled":false}
wait 2500
$STATUS
eval content $(probe false)
wait 18000
eval content window.__r
cmd {"cmd":"set_extension_enabled","id":"$ID","enabled":true}
wait 2500
$STATUS
cmd {"cmd":"remove_catalog","id":"$ID"}
wait 2000
$STATUS
quit
S
out=$(timeout 240 cargo run -q -p blackhole --features devtools 2>&1 | grep 'dev\] eval ->' || true)
mapfile -t L <<<"$out"
fail=0
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
check 0 "tracker blocker" "catalog lists the extension (available)"
check 1 "Active" "install: downloaded, verified, loaded"
check 3 'amazon-adsystem.com\/aax2\/apstag.js\":\"NETERR' "enabled: ad host blocked by DNR"
check 3 'newrelic.com\/nr-loader-spa-current.min.js\":\"NETERR' "enabled: second tracker blocked by DNR"
check 3 'example.org\/\":\"ok' "enabled: control URL loads"
check 4 "Turned off" "disable: status off"
check 6 'amazon-adsystem.com\/aax2\/apstag.js\":\"ok' "disabled: ad host reachable again"
check 7 "Active" "re-enable: active again"
check 8 "tracker blocker" "remove: back to available"
exit $fail
