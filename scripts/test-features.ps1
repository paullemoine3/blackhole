# UI scenarios of clean links, the black hole tab, the pilot split, highlights and Recall on
# Windows, driven by the devtools harness (see crates/app/src/devtools.rs and scripts/test-ui.ps1).
# Uses the scratch profile %TEMP%\bh_data (wiped first); screenshots land in %TEMP%\bh_shots.
# Needs network access (example.com, iana.org, wikipedia.org) and a visible, uncovered desktop.
# Every eval is named ("cl1:", "bh2:"…) and read by its name: adding one shifts nothing.
# The Wormhole needs two paired devices: it is covered by the loopback test of
# blackhole-infrastructure (handoff::tests), not here.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue

$script = @'
wait 3000
cmd {"cmd":"set_language","language":"en"}
# ---- clean links ----
cmd {"cmd":"open","input":"https://example.com/?utm_source=mail&id=3&fbclid=abc"}
wait 5000
eval content 'cl1:' + location.href
eval content (document.cookie = 'bhx=1; max-age=600; Secure', 'cl2:' + document.cookie.includes('bhx'))
cmd {"cmd":"open","input":"https://www.google.com/url?q=https%3A%2F%2Fexample.org%2F%3Futm_medium%3Dx&sa=D"}
wait 5000
eval content 'cl3:' + location.href
cmd {"cmd":"open_settings","tab":"privacy"}
wait 2500
eval settings 'st1:' + document.querySelectorAll('#shields .opt').length + '|' + [...document.querySelectorAll('#shields .label')].map(l => l.textContent).join(',')
eval settings 'st2:' + document.querySelector('#recall .opt').getAttribute('aria-checked') + '|' + document.getElementById('recallclear').textContent.trim()
shots f01-settings-privacy
cmd {"cmd":"set_shield","name":"clean_links","on":false}
wait 600
eval settings 'st3:' + [...document.querySelectorAll('#shields .opt')].map(o => o.getAttribute('aria-checked')).join(',')
cmd {"cmd":"set_shield","name":"clean_links","on":true}
cmd {"cmd":"set_recall","on":true}
wait 600
eval settings 'st4:' + document.querySelector('#recall .opt').getAttribute('aria-checked')
cmd {"cmd":"close_settings"}
wait 600
# ---- black hole tab ----
cmd {"cmd":"palette","mode":"new"}
wait 1200
cmd {"cmd":"action","id":"burner_tab"}
wait 1500
eval palette 'bh1:' + document.getElementById('q').placeholder
shot f02-burner-palette
cmd {"cmd":"open","input":"https://example.com/?hole=1"}
wait 5000
eval chrome 'bh2:' + state.tabs.filter(t => t.burner).length + '|' + document.querySelectorAll('.tab .burn').length + '|' + state.tabs.length
eval content 'bh3:' + location.href + '|' + document.cookie.includes('bhx')
shot f03-burner-tab
eval chrome (send({ cmd: 'pin', id: state.tabs.find(t => t.burner).id }), 'bh4:sent')
wait 600
eval chrome 'bh5:' + state.tabs.filter(t => t.burner && t.pinned).length
eval chrome (send({ cmd: 'close', id: state.tabs.find(t => t.burner).id }), 'bh6:sent')
wait 1500
eval chrome 'bh7:' + state.tabs.filter(t => t.burner).length + '|' + state.tabs.length
# ---- pilot split ----
cmd {"cmd":"open","input":"https://www.iana.org/help/example-domains"}
wait 6000
eval chrome (send({ cmd: 'split', id: state.tabs.find(t => t.url.includes('example.org')).id }), 'pl0:sent')
wait 2500
cmd {"cmd":"pilot"}
wait 800
eval splitbar 'pl1:' + document.querySelectorAll('button.pilot').length + '|' + document.querySelectorAll('button.pilot.on').length + '|' + document.querySelector('.pane button.pilot').getAttribute('aria-pressed')
shot f04-pilot-on
eval content (document.querySelector('a[href*="rfc2606"], a[href*="rfc"]').click(), 'pl2:clicked')
wait 7000
eval chrome 'pl3:' + state.tabs.find(t => t.id === state.activeTab).url + ' >> ' + state.tabs.filter(t => t.split && t.id !== state.activeTab).map(t => t.url).join(',')
shot f05-pilot-link-in-other-pane
cmd {"cmd":"pilot"}
wait 600
eval splitbar 'pl4:' + document.querySelectorAll('button.pilot.on').length
cmd {"cmd":"close_split"}
wait 1200
# ---- highlights ----
eval content (() => { const p = [...document.querySelectorAll('p')].find(p => p.textContent.trim().length > 60); const r = document.createRange(); r.selectNodeContents(p); getSelection().removeAllRanges(); getSelection().addRange(r); return 'hl0:' + String(getSelection()).trim().length; })()
cmd {"cmd":"highlight_selection"}
wait 2000
eval content (getSelection().removeAllRanges(), 'hl1:' + (CSS.highlights.has('bh-mark') ? CSS.highlights.get('bh-mark').size : 0))
shot f06-highlight
cmd {"cmd":"reload"}
wait 6000
eval content 'hl2:' + (CSS.highlights.has('bh-mark') ? CSS.highlights.get('bh-mark').size : 0)
cmd {"cmd":"open_library","section":"highlights"}
wait 3000
eval library 'hl3:' + document.querySelector('h1').textContent + '|' + document.querySelectorAll('.row').length + '|' + document.querySelector('.nav.on').dataset.s
shot f07-library-highlights
eval library (document.querySelector('.row .acts button').click(), 'hl4:' + !!document.getElementById('note'))
wait 500
eval library (() => { const n = document.getElementById('note'); n.value = 'Reserved by RFC 2606'; n.dispatchEvent(new Event('input')); n.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); return 'hl5:typed'; })()
wait 1000
eval library 'hl6:' + document.querySelector('.row i').textContent
shot f08-library-highlight-note
cmd {"cmd":"close_library"}
wait 1500
eval content 'hl7:' + (CSS.highlights.has('bh-mark-note') ? CSS.highlights.get('bh-mark-note').size : 0)
# ---- Recall ----
cmd {"cmd":"open","input":"https://en.wikipedia.org/wiki/Black_hole"}
wait 14000
cmd {"cmd":"shortcut","key":"shift+f"}
wait 1500
eval palette (document.getElementById('q').value = 'event horizon', document.getElementById('q').dispatchEvent(new Event('input')), 'rc0:' + S.mode + '|' + document.getElementById('q').placeholder)
wait 2500
eval palette 'rc1:' + items.length + '|' + (items[0] ? items[0].title + '|' + items[0].sub.slice(0, 80) : '')
shot f09-recall-results
eval palette (document.getElementById('q').value = 'zzqqxx nothing', document.getElementById('q').dispatchEvent(new Event('input')), 'rc2:typed')
wait 2000
eval palette 'rc3:' + items.length + '|' + document.querySelector('#list .none').textContent
cmd {"cmd":"palette_close"}
wait 600
cmd {"cmd":"open_settings","tab":"privacy"}
wait 2500
eval settings 'rc4:' + document.getElementById('recallcount').textContent
cmd {"cmd":"recall_exclude","site":"https://en.wikipedia.org/wiki/X","on":true}
wait 1500
eval settings 'rc5:' + [...document.querySelectorAll('#recallsites .name')].map(n => n.textContent).join(',') + '|' + document.getElementById('recallcount').textContent
eval settings (document.getElementById('recall').closest('.card').scrollIntoView(), 'rc6:scrolled')
wait 500
shots f10-settings-recall
cmd {"cmd":"close_settings"}
wait 600
quit
'@
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii

$logFile = Join-Path $tmp 'bh_run.log'
# Redirecting to a file (rather than capturing the pipe) keeps the app from blocking on its output.
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(540000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
$log = Get-Content $logFile
$all = @($log | Where-Object { $_ -match '\[dev\] eval ->' })
function Ev($name) { [string]($all | Where-Object { $_ -match "-> `"${name}:" } | Select-Object -First 1) }
$saved = @($log | Where-Object { $_ -match '\[dev\] saved' })
$log | Where-Object { $_ -match '\[dev\] (bad|unknown|eval failed|screenshot failed)' } | ForEach-Object { Write-Host $_ }

$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
Check ((Ev cl1) -match 'cl1:https://example\.com/\?id=3"') 'clean links: tracking parameters dropped, the others kept'
Check ((Ev cl3) -match 'cl3:https://example\.org/"') 'clean links: the redirect page of a search engine is skipped and its target cleaned'
Check ((Ev st1) -match 'st1:5\|.*Clean links') 'settings: the Clean links shield is listed with the others'
Check ((Ev st2) -match 'st2:false\|Forget everything') 'settings: Recall is off at first'
Check ((Ev st3) -match 'st3:true,true,true,true,false') 'settings: the Clean links switch follows the setting'
Check ((Ev st4) -match 'st4:true') 'settings: Recall switches on'
Check ((Ev bh1) -match 'bh1:.*black hole tab') 'black hole tab: the command bar says the next address opens in one'
Check ((Ev bh2) -match 'bh2:1\|1\|3') 'black hole tab: opened beside the tab of the same address, marked in the sidebar'
Check ((Ev cl2) -match 'cl2:true' -and (Ev bh3) -match 'bh3:https://example\.com/\?hole=1\|false') 'black hole tab: it does not see the cookies of the space'
Check ((Ev bh5) -match 'bh5:0') 'black hole tab: it cannot join quick access'
Check ((Ev bh7) -match 'bh7:0\|2') 'black hole tab: closed, it is gone'
Check ((Ev pl1) -match 'pl1:2\|1\|true') 'pilot split: a button on each pane, the active pane pilots'
Check ((Ev pl3) -match 'pl3:https://www\.iana\.org/help/example-domains >> https?://[^,]*rfc') 'pilot split: a link clicked in the pilot pane opens in the other pane'
Check ((Ev pl4) -match 'pl4:0') 'pilot split: it stops'
Check ((Ev hl0) -notmatch 'hl0:0"' -and (Ev hl1) -match 'hl1:1') 'highlights: the selection is highlighted'
Check ((Ev hl2) -match 'hl2:1') 'highlights: found again after the page is loaded again'
Check ((Ev hl3) -match 'hl3:Highlights\|1\|highlights') 'highlights: listed in the Library'
Check ((Ev hl4) -match 'hl4:true' -and (Ev hl6) -match 'hl6:Reserved by RFC 2606') 'highlights: a note is written on its row'
Check ((Ev hl7) -match 'hl7:1') 'highlights: a highlight with a note is drawn as such in the page'
Check ((Ev rc0) -match 'rc0:recall\|A few words') 'Recall: the command bar opens in its mode'
Check ((Ev rc1) -match 'rc1:[1-9]\d*\|Black hole') 'Recall: a page read is found from words of its text'
Check ((Ev rc3) -match 'rc3:0\|No ') 'Recall: no match, no result'
Check ((Ev rc4) -match 'rc4:[1-9]\d* pages? remembered') 'Recall: Settings count the pages remembered'
Check ((Ev rc5) -match 'rc5:en\.wikipedia\.org\|') 'Recall: an excluded site is listed'
$burnt = @(Get-ChildItem (Join-Path $tmp 'bh_data\webview') -Recurse -Directory -Filter 'BlackHole-burn-*' -ErrorAction SilentlyContinue)
Check ($burnt.Count -eq 0) "black hole tab: no engine profile left on disk ($($burnt.Count))"
$clear = @(Get-ChildItem (Join-Path $tmp 'bh_data\recall') -File -ErrorAction SilentlyContinue | Where-Object { (Get-Content $_.FullName -Raw) -match 'horizon' })
Check ((Test-Path (Join-Path $tmp 'bh_data\recall')) -and $clear.Count -eq 0) 'Recall: its files exist and hold no readable text'
Check ($saved.Count -eq 10) "screenshots saved ($($saved.Count)/10)"
if ($fail) { Write-Host "evals:"; $all }
exit [int]$fail
