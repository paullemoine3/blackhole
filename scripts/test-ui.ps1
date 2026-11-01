# UI scenarios on Windows, driven by the devtools harness (see crates/app/src/devtools.rs).
# Uses the scratch profile %TEMP%\bh_data (wiped first); screenshots land in %TEMP%\bh_shots.
# Needs network access and a visible, uncovered desktop (the window is captured from the screen).
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue

$script = @'
wait 3000
shot 01-home
eval chrome document.body ? 'ready' : 'empty'
cmd {"cmd":"open","input":"https://example.com"}
wait 5000
shot 02-example-com
eval content document.title
cmd {"cmd":"navigate","input":"https://example.org"}
wait 5000
shot 03-second-page
eval content location.href
cmd {"cmd":"back"}
wait 1500
eval content location.href
cmd {"cmd":"palette","mode":"new"}
wait 1500
shot 04-palette
cmd {"cmd":"palette_close"}
wait 500
cmd {"cmd":"open_settings","tab":"general"}
wait 2000
shots 05-settings-general
cmd {"cmd":"open_settings","tab":"shortcuts"}
wait 1000
shots 06-settings-shortcuts
cmd {"cmd":"open_settings","tab":"extensions"}
wait 1500
shots 07-settings-extensions
eval settings document.querySelector('#catalog') ? 'catalog' : 'missing'
cmd {"cmd":"open_settings","tab":"import"}
wait 1000
shots 08-settings-import
cmd {"cmd":"open_settings","tab":"privacy"}
wait 1000
shots 09-settings-privacy
cmd {"cmd":"close_settings"}
wait 500
cmd {"cmd":"home"}
wait 1000
shot 10-home-again
cmd {"cmd":"set_animations","on":true}
cmd {"cmd":"hover","on":true}
wait 80
eval chrome window.innerWidth
shot 11-sidebar-mid-expand
wait 700
eval chrome window.innerWidth
shot 12-sidebar-expanded
cmd {"cmd":"hover","on":false}
wait 700
eval chrome window.innerWidth
cmd {"cmd":"palette","mode":"new"}
wait 100
eval palette document.body.className
wait 1800
eval palette document.body.className
shot 13-palette-open
cmd {"cmd":"palette_close"}
wait 400
eval palette document.body.className
cmd {"cmd":"set_animations","on":false}
cmd {"cmd":"palette","mode":"new"}
wait 400
eval palette document.body.className
cmd {"cmd":"palette_close"}
wait 300
cmd {"cmd":"set_animations","on":true}
cmd {"cmd":"hover","on":true}
wait 700
eval chrome (bh.render({...state, update:{state:'available',version:'9.9.9'}}), document.getElementById('update').hidden + '|' + document.getElementById('updtxt').textContent)
wait 300
shot 14-update-available
eval chrome (document.getElementById('update').classList.add('hov'), 'x')
wait 350
eval chrome getComputedStyle(document.querySelector('#update .ico')).animationName
shot 15-update-hover
eval chrome (bh.render({...state, update:{state:'downloading',version:'9.9.9'}}), document.getElementById('updtxt').textContent)
wait 300
shot 16-update-downloading
eval chrome (bh.render({...state, update:null}), String(document.getElementById('update').hidden))
cmd {"cmd":"hover","on":false}
wait 300
quit
'@
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii

$logFile = Join-Path $tmp 'bh_run.log'
# Redirecting to a file (rather than capturing the pipe) keeps the app from blocking on its output.
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(240000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
$log = Get-Content $logFile
$evals = @($log | Where-Object { $_ -match '\[dev\] eval ->' })
$saved = @($log | Where-Object { $_ -match '\[dev\] saved' })
$log | Where-Object { $_ -match '\[dev\] (bad|unknown|eval failed|screenshot failed)' } | ForEach-Object { Write-Host $_ }

$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
Check ($evals[0] -match 'ready') 'chrome page loads'
Check ($evals[1] -match 'Example Domain') 'navigation: example.com opens'
Check ($evals[2] -match 'example\.org') 'navigate: example.org loads'
Check ($evals[3] -match 'example\.com') 'back: returns to example.com'
Check ($evals[4] -match 'catalog') 'settings: extension catalog present'
function Width($line) { if ($line -match '(\d+(\.\d+)?)\s*$') { [double]$Matches[1] } else { -1 } }
$mid = Width $evals[5]; $full = Width $evals[6]; $rail = Width $evals[7]
Check ($mid -gt 64 -and $mid -lt 280) "sidebar animation: mid-expand width is between rail and full ($mid)"
Check ($full -eq 280) "sidebar: hover expands to full width ($full)"
Check ($rail -eq 64) "sidebar: leaving collapses to the rail ($rail)"
Check ($evals[8] -match 'open' -and $evals[8] -match 'pre|pop|intro') 'palette animation: opening plays the intro/pop classes'
Check ($evals[9] -match 'open') 'palette: open after animation'
Check ($evals[10] -notmatch 'open') 'palette: closes'
Check ($evals[11] -match 'open' -and $evals[11] -notmatch 'pre|pop|intro') 'animations off: palette opens without animation classes'
Check ($evals[12] -match 'false\|Update to v9\.9\.9') 'update button: shown with the new version'
Check ($evals[14] -match 'up-bounce') 'update button: hover animation runs'
Check ($evals[15] -match 'Updating to v9\.9\.9') 'update button: downloading state'
Check ($evals[16] -match 'true') 'update button: hidden when up to date'
Check ($saved.Count -eq 16) "screenshots saved ($($saved.Count)/16)"
if ($fail) { Write-Host "evals:"; $evals }
exit [int]$fail
