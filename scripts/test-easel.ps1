# Easel scenarios on Windows, driven by the devtools harness (see crates/app/src/devtools.rs):
# create, capture a page, notes, edits, link back to the page, rename, delete, live capture (pick an
# area, play it, check the crop), persistence.
# Uses the scratch profile %TEMP%\bh_data (wiped first); screenshots land in %TEMP%\bh_shots.
# Needs network access and a visible, uncovered desktop (the window is captured from the screen).
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue

$script = @'
wait 3000
cmd {"cmd":"open","input":"https://example.com"}
wait 5000
cmd {"cmd":"new_easel","name":"Ideas"}
cmd {"cmd":"open_easel"}
wait 2000
eval easel document.getElementById('title').value
cmd {"cmd":"capture_to_easel"}
wait 1500
eval easel JSON.stringify([...document.querySelectorAll('.card.clip .link')].map(a => a.textContent))
eval easel document.querySelector('.card.clip .host').textContent
cmd {"cmd":"easel_add_note"}
wait 500
eval easel document.querySelectorAll('.card').length
eval easel (() => { const t = document.querySelector('.card.note textarea'); t.value = 'hello easel'; t.dispatchEvent(new Event('input')); return 'typed'; })()
wait 600
cmd {"cmd":"easel_move","item":2,"x":420,"y":300}
cmd {"cmd":"easel_color","item":2,"color":"#2e8b6e"}
wait 500
shote 01-easel-board
cmd {"cmd":"home"}
wait 800
cmd {"cmd":"capture_to_easel"}
wait 800
eval easel document.querySelectorAll('.card').length
cmd {"cmd":"open","input":"https://example.org"}
wait 4000
cmd {"cmd":"easel_open_link","item":1}
wait 4000
eval content location.href
cmd {"cmd":"rename_easel","id":1,"name":"Renamed"}
cmd {"cmd":"new_easel","name":"Second"}
wait 500
eval easel JSON.stringify([...document.querySelectorAll('.easel .name')].map(n => n.textContent))
shote 02-easel-two-boards
cmd {"cmd":"delete_easel","id":2}
wait 500
eval easel document.querySelectorAll('.easel').length
cmd {"cmd":"easel_remove","item":2}
wait 500
eval easel document.querySelectorAll('.card').length
cmd {"cmd":"open","input":"https://example.com"}
wait 4000
cmd {"cmd":"live_pick"}
wait 500
eval content String(window.__bhPick === true)
eval content (() => { const h = document.querySelector('p').getBoundingClientRect(); const x = h.left + h.width / 2, y = h.top + h.height / 2; for (const t of ['mousemove', 'click']) document.elementFromPoint(x, y).dispatchEvent(new MouseEvent(t, { bubbles: true, cancelable: true, clientX: x, clientY: y, view: window })); return 'picked'; })()
wait 1500
eval easel JSON.stringify([...document.querySelectorAll('.card.live .slot')].map(c => c.textContent))
cmd {"cmd":"easel_live_play","item":2}
wait 8000
eval easel document.querySelector('.card.live .play').textContent
eval live (() => { const r = document.querySelector('p').getBoundingClientRect(); return 'crop ' + (Math.abs(r.left) < 2 && Math.abs(r.top) < 2 && r.width <= innerWidth + 3 && r.height <= innerHeight + 3); })()
shote 03-easel-live
cmd {"cmd":"easel_live_stop","item":2}
wait 500
eval easel document.querySelector('.card.live .play').textContent
cmd {"cmd":"new_folder"}
wait 600
eval chrome (() => { const i = document.querySelector('.fedit'); if (i) i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' })); return 'x'; })()
wait 600
eval chrome (() => { const n = document.querySelector('#easels .tab'); const f = document.querySelector('.fh'); const a = n.getBoundingClientRect(); let b; const ev = (t, el, x, y) => el.dispatchEvent(new MouseEvent(t, { bubbles: true, cancelable: true, button: 0, clientX: x, clientY: y, view: window })); ev('mousedown', n, a.left + 20, a.top + 10); ev('mousemove', document, a.left + 60, a.top + 10); b = f.getBoundingClientRect(); ev('mousemove', document, b.left + 20, b.top + b.height / 2); ev('mouseup', document, b.left + 20, b.top + b.height / 2); return 'dragged'; })()
wait 1000
eval chrome JSON.stringify({ loose: document.querySelectorAll('#easels .tab').length, inFolder: document.querySelectorAll('.fb .tab.easel').length })
eval chrome (() => { document.querySelector('.fb .tab.easel').click(); return 'clicked'; })()
wait 1500
eval etab JSON.stringify({ embedded: document.body.classList.contains('embedded'), nav: getComputedStyle(document.querySelector('nav')).display, cards: document.querySelectorAll('.card').length, name: document.getElementById('title').value })
eval chrome JSON.stringify({ home: state.home, open: state.easels.map(e => e.open) })
cmd {"cmd":"easel_live_play","item":2}
wait 8000
eval live (() => { const r = document.querySelector('p').getBoundingClientRect(); return 'tabcrop ' + (Math.abs(r.left) < 2 && Math.abs(r.top) < 2 && r.width <= innerWidth + 3 && r.height <= innerHeight + 3 && innerWidth > 50); })()
shot 04-easel-tab
cmd {"cmd":"close_easel_tab"}
wait 800
eval chrome JSON.stringify({ home: state.home, open: state.easels.map(e => e.open) })
eval chrome JSON.stringify({ before: document.getElementById('bms').nextElementSibling.id, after: document.getElementById('bms').previousElementSibling.className, bmhead: !!document.getElementById('bmhead') })
quit
'@
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii

$logFile = Join-Path $tmp 'bh_run.log'
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(240000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
$log = Get-Content $logFile
$evals = @($log | Where-Object { $_ -match '\[dev\] eval ->' })
$log | Where-Object { $_ -match '\[dev\] (bad|unknown|eval failed|screenshot failed)' } | ForEach-Object { Write-Host $_ }

$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
Check ($evals[0] -match 'Ideas') 'easel: created and shown'
Check ($evals[1] -match 'Example Domain') 'capture: clip carries the page title'
Check ($evals[2] -match 'example\.com') 'capture: clip shows its source host'
Check ($evals[3] -match '2') 'note: added next to the clip'
Check ($evals[4] -match 'typed') 'note: text typed'
Check ($evals[5] -match '2') 'capture: home page is refused (still 2 cards)'
Check ($evals[6] -match 'example\.com') 'clip link: opens the captured page in a tab'
Check ($evals[7] -match 'Renamed' -and $evals[7] -match 'Second') 'rename / new easel: both listed'
Check ($evals[8] -match '1') 'delete: only one easel left'
Check ($evals[9] -match '1') 'remove: note deleted, one card left'
Check ($evals[10] -match 'true') 'live: picker armed on the page'
Check ($evals[11] -match 'picked') 'live: area clicked'
Check ($evals[12] -match 'Paused') 'live: card added, paused until played'
Check ($evals[13] -match ([string][char]0x23F8)) 'live: playing'
Check ($evals[14] -match 'crop true') 'live: view shows the picked area, scaled to fill the card'
Check ($evals[15] -match ([string][char]0x25B6)) 'live: paused again'
Check ($evals[17] -match 'dragged') 'sidebar: easel dragged onto a folder'
Check ($evals[18] -match 'inFolder.:1') 'sidebar: easel now lives in the folder'
Check ($evals[19] -match 'clicked') 'tab: easel opened from the sidebar'
Check ($evals[20] -match 'embedded.:true') 'tab: easel page is embedded in the main window'
Check ($evals[20] -match 'nav.:.none') 'tab: the list of easels is hidden'
Check ($evals[20] -match 'name.:.Renamed') 'tab: shows the clicked easel'
Check ($evals[21] -match 'open.:\[true\]') 'tab: sidebar marks the easel open'
Check ($evals[22] -match 'tabcrop true') 'tab: live card plays cropped inside the content area'
Check ($evals[23] -match 'open.:\[false\]') 'tab: closing returns to the page'
Check ($evals[24] -match 'before.:.folders') 'bookmarks: sit in the Pinned section, ahead of folders and tabs'
Check ($evals[24] -match 'bmhead.:false') 'bookmarks: no separate collapsed section anymore'
Check ((Test-Path (Join-Path $tmp 'bh_shots\01-easel-board.png')) -and (Test-Path (Join-Path $tmp 'bh_shots\03-easel-live.png'))) 'screenshots saved'
$file = Join-Path $tmp 'bh_data\easels.json'
$data = if (Test-Path $file) { @(Get-Content $file -Raw | ConvertFrom-Json) } else { @() }
$ok = $data.Count -eq 1 -and $data[0].items.Count -eq 2
$clip = if ($ok) { $data[0].items[0] } else { $null }
$live = if ($ok) { $data[0].items[1] } else { $null }
Check ($ok -and $data[0].name -eq 'Renamed' -and $clip.kind -eq 'clip' -and $clip.url -like 'https://example.com*' -and $live.kind -eq 'live' -and $live.selector -like '*p:nth-of-type*' -and $live.url -like 'https://example.com*') 'persistence: easels.json holds the renamed easel, its clip and the live area'
if ($fail) { Write-Host 'evals:'; $evals }
exit [int]$fail
