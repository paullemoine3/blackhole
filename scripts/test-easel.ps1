# Easel scenarios on Windows, driven by the devtools harness (see crates/app/src/devtools.rs):
# create, capture a live page, notes, edits, link back to the page, rename, delete, persistence.
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
Check (Test-Path (Join-Path $tmp 'bh_shots\01-easel-board.png')) 'screenshot saved'
$file = Join-Path $tmp 'bh_data\easels.json'
$data = if (Test-Path $file) { @(Get-Content $file -Raw | ConvertFrom-Json) } else { @() }
$clip = if ($data.Count -eq 1 -and $data[0].items.Count -eq 1) { $data[0].items[0] } else { $null }
Check ($clip -and $data[0].name -eq 'Renamed' -and $clip.kind -eq 'clip' -and $clip.url -like 'https://example.com*') 'persistence: easels.json holds the renamed easel and its clip'
if ($fail) { Write-Host 'evals:'; $evals }
exit [int]$fail
