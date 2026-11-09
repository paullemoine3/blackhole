#!/usr/bin/env bash
# Settings window and icon scale on macOS, driven by the devtools harness: every Settings section
# is captured (/tmp/bh_shots/<prefix>settings-*.png, plus the sidebar and the Easel window), then
# the checks read the page: design tokens and icons present, switches, their intermediate state,
# the animations-off mode, keyboard navigation of the sections, empty states.
# Scratch profile /tmp/bh_data (wiped first); no network needed. Keep the window visible.
#   BH_SHOTS_ONLY=1 BH_PREFIX=before- bash scripts/test-settings.sh   # captures only
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
P=${BH_PREFIX:-}
{
  echo "wait 2500"
  echo "cmd {\"cmd\":\"set_language\",\"language\":\"en\"}"
  echo "shot ${P}sidebar"
  for tab in general shortcuts extensions calendar devices passwords import privacy; do
    echo "cmd {\"cmd\":\"open_settings\",\"tab\":\"$tab\"}"
    echo "wait 1200"
    echo "shots ${P}settings-$tab"
  done
  if [[ -z "${BH_SHOTS_ONLY:-}" ]]; then
    cat <<'S'
eval settings 'tok:' + getComputedStyle(document.documentElement).getPropertyValue('--bh-radius-card').trim() + '|' + getComputedStyle(document.documentElement).getPropertyValue('--bh-icon-sm').trim()
eval settings 'nav:' + document.querySelectorAll('nav button svg.bh-i').length + '|' + getComputedStyle(document.querySelector('nav button svg')).width + '|' + getComputedStyle(document.getElementById('top')).height
eval settings 'sw:' + [...document.querySelectorAll('#shields .opt')].map(o => (o.classList.contains('sel') ? 1 : 0) + (o.getAttribute('aria-checked') === String(o.classList.contains('sel')) ? '' : '!')).join('')
cmd {"cmd":"set_shield","name":"hide_ads","on":false}
wait 500
eval settings 'sw2:' + [...document.querySelectorAll('#shields .opt')].map(o => o.classList.contains('sel') ? 1 : 0).join('')
cmd {"cmd":"set_shield","name":"hide_ads","on":true}
wait 400
eval settings 'mid:' + (() => { document.querySelector('#shields .opt').classList.remove('sel'); return 'off'; })()
wait 500
eval settings 'mid1:' + (() => { const o = document.querySelector('#shields .opt'); const k = o.querySelector('.switch i'); o.classList.add('sel'); setTimeout(() => { window.__mid = new DOMMatrix(getComputedStyle(k).transform).m41.toFixed(1); }, 60); return 'on'; })()
wait 300
eval settings 'mid2:' + window.__mid
cmd {"cmd":"set_animations","on":false}
wait 500
eval settings 'off:' + document.body.classList.contains('noanim') + '|' + getComputedStyle(document.querySelector('#shields .opt .switch i')).transitionDuration
cmd {"cmd":"set_animations","on":true}
wait 400
eval settings 'kb:' + (() => { const n = document.querySelector('nav button.on'); n.focus(); n.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true })); return document.querySelector('section.on').id + '|' + document.activeElement.dataset.tab; })()
eval settings 'empty:' + !!document.querySelector('#blocklist .empty svg.bh-i') + '|' + document.querySelector('#blocklist .empty').textContent.trim()
eval chrome 'side:' + ['library', 'gear', 'shield', 'addspace', 'new'].map(id => document.querySelectorAll('#' + id + ' svg.bh-i').length).join('')
eval bar 'bar:' + ['sidebar', 'back', 'forward', 'reload', 'home'].map(id => document.querySelectorAll('#' + id + ' svg.bh-i').length).join('')
cmd {"cmd":"open_settings","tab":"privacy"}
wait 1000
eval settings 'focus:' + (() => { const b = document.getElementById('clear'); b.focus(); return getComputedStyle(b).outlineStyle + '|' + b.matches(':focus-visible'); })()
shots ${P}settings-focus
S
  fi
  echo "cmd {\"cmd\":\"close_settings\"}"
  echo "wait 600"
  echo "cmd {\"cmd\":\"open_easel\"}"
  echo "wait 1500"
  echo "shote ${P}easel"
  echo "quit"
} | sed "s/\${P}/$P/g" > /tmp/bh_script.txt

out=$(timeout 180 cargo run -q -p blackhole --features devtools 2>&1 | grep 'dev\] eval ->' || true)
[[ -n "${BH_SHOTS_ONLY:-}" ]] && { ls /tmp/bh_shots; exit 0; }
mapfile -t L <<<"$out"
fail=0
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
check 0 'tok:12px|16px' "design tokens injected (--bh-*)"
check 1 'nav:8|16px|38px' "section icons at 16 px, top bar 38 px"
check 2 'sw:1111' "shield switches on, aria-checked in step"
check 3 'sw2:1011' "a switch follows the setting"
check 6 'mid2:' "switch knob read mid-way"
if [[ "${L[6]:-}" =~ mid2:([0-9.]+) ]] && awk "BEGIN{v=${BASH_REMATCH[1]}; exit !(v > 0.5 && (v < 11.9 || v > 12.1))}"; then echo "ok   knob in motion after 60 ms (neither end, spring overshoot allowed)"; else echo "FAIL knob not mid-way (got: ${L[6]:-none})"; fail=1; fi
check 7 'off:true|0s' "animations off: no transition"
check 8 'kb:passwords|passwords' "arrow keys move between sections (Privacy → Passwords)"
check 9 'empty:true|Nothing blocked yet' "empty state with its icon"
check 10 'side:11111' "sidebar icons drawn (no emoji)"
check 11 'bar:11111' "top bar icons drawn (no font glyphs)"
check 12 'focus:solid|true' "keyboard focus ring"
for s in general shortcuts extensions calendar devices passwords import privacy focus; do
  [[ -f /tmp/bh_shots/${P}settings-$s.png ]] || { echo "FAIL screenshot settings-$s missing"; fail=1; }
done
exit $fail
