#!/usr/bin/env bash
# Interface language on macOS, driven by the devtools harness: English, then French chosen in
# Settings > General, applied live (no restart) to the sidebar, the top bar, Settings, the Easel
# window, the Library and the next error page; then back to English. Checks read the pages' text.
# Scratch profile /tmp/bh_data (wiped first); no network needed (the error page comes from a
# closed local port). Screenshots in /tmp/bh_shots/i18n-*.png. Keep the window visible.
set -euo pipefail
cd "$(dirname "$0")/.."
rm -rf /tmp/bh_data /tmp/bh_shots
cat > /tmp/bh_script.txt <<'S'
wait 2500
cmd {"cmd":"set_language","language":"en"}
wait 600
cmd {"cmd":"open_settings","tab":"general"}
wait 1200
eval settings 'en:' + document.querySelector('#general h1').textContent + '|' + document.documentElement.lang + '|' + document.querySelector('#langs .opt.sel .label').textContent
eval chrome 'enside:' + document.querySelector('#new .full-only').textContent + '|' + document.getElementById('library').title
shots i18n-en-settings
eval settings 'pick:' + (() => { [...document.querySelectorAll('#langs .opt')].find(o => o.textContent === 'Français').click(); return 'fr'; })()
wait 1200
eval settings 'fr:' + document.querySelector('#general h1').textContent + '|' + document.documentElement.lang + '|' + document.querySelector('nav button[data-tab=privacy]').textContent.trim() + '|' + document.querySelector('#langs .opt.sel .label').textContent + '|' + document.getElementById('where').textContent
eval chrome 'frside:' + document.querySelector('#new .full-only').textContent + '|' + document.getElementById('library').title + '|' + document.getElementById('stats').textContent
eval bar 'frbar:' + document.getElementById('url').textContent + '|' + document.getElementById('home').title
eval settings 'frsleep:' + document.querySelector('#sleep .label').textContent + '|' + document.querySelector('#sleep .seg button').textContent
shots i18n-fr-settings
cmd {"cmd":"open_settings","tab":"passwords"}
wait 800
shots i18n-fr-passwords
cmd {"cmd":"open_settings","tab":"shortcuts"}
wait 800
shots i18n-fr-shortcuts
cmd {"cmd":"close_settings"}
wait 600
shot i18n-fr-main
cmd {"cmd":"open_easel"}
wait 1500
eval easel 'freasel:' + document.getElementById('new').textContent + '|' + document.querySelector('#capture span').textContent
shote i18n-fr-easel
cmd {"cmd":"close_easel"}
wait 600
cmd {"cmd":"open_library"}
wait 1500
eval library 'frlib:' + document.querySelector('#m h1').textContent + '|' + document.querySelector('.nav[data-s=history] span').textContent
shot i18n-fr-library
cmd {"cmd":"close_library"}
wait 600
cmd {"cmd":"open","input":"http://127.0.0.1:47999/"}
wait 5000
eval content 'frerr:' + document.querySelector('h1').textContent + '|' + document.documentElement.lang
shot i18n-fr-error
cmd {"cmd":"set_language","language":"en"}
wait 800
cmd {"cmd":"open","input":"http://127.0.0.1:47999/x"}
wait 5000
eval content 'enerr:' + document.querySelector('h1').textContent
eval chrome 'back:' + document.querySelector('#new .full-only').textContent + '|' + document.documentElement.lang
shot i18n-en-main
eval chrome 'alive'
quit
S
out=$(timeout 240 cargo run -q -p blackhole --features devtools 2>&1 | grep 'dev\] eval ->' || true)
mapfile -t L <<<"$out"
fail=0
check() { if [[ "${L[$1]:-}" == *"$2"* ]]; then echo "ok   $3"; else echo "FAIL $3 (got: ${L[$1]:-none})"; fail=1; fi; }
check 0 'en:General|en|English' "English: Settings in English, the choice is marked"
check 1 'enside:New tab|Library' "English: sidebar texts"
check 2 'pick:fr' "French picked in Settings > General"
check 3 'fr:Général|fr|Confidentialité|Français| · Général' "French: Settings re-rendered live (markup, nav, choice, title)"
check 4 'frside:Nouvel onglet|Bibliothèque|' "French: sidebar updated without a restart"
check 5 'frbar:Rechercher avec' "French: top bar placeholder"
check 6 'frsleep:Mettre en veille les onglets inutilisés après|5 min' "French: rendered options"
check 7 'freasel:+ Nouvel Easel|Capturer la page' "French: Easel window"
check 8 'frlib:Médias|Historique' "French: Library"
check 9 'frerr:Site injoignable|fr' "French: the next error page"
check 10 'enerr:Site unreachable' "English again: error page"
check 11 'back:New tab|en' "English again: sidebar"
check 12 'alive' "no crash: the app is still running"
for s in en-settings fr-settings fr-passwords fr-shortcuts fr-main fr-easel fr-library fr-error en-main; do
  [[ -f /tmp/bh_shots/i18n-$s.png ]] || { echo "FAIL screenshot i18n-$s missing"; fail=1; }
done
exit $fail
