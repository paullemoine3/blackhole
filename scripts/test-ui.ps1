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
cmd {"cmd":"lock"}
wait 80
eval home window.innerWidth
shot 11-sidebar-mid-hide
wait 700
eval home window.innerWidth
shot 12-sidebar-hidden
cmd {"cmd":"peek","on":true}
wait 500
eval home window.innerWidth
shot 12b-sidebar-flyout
cmd {"cmd":"peek","on":false}
wait 500
cmd {"cmd":"lock"}
wait 700
eval home window.innerWidth
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
wait 300
cmd {"cmd":"window_maximize"}
wait 800
eval bar document.body.className
cmd {"cmd":"window_maximize"}
wait 800
eval bar document.body.className
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
$mid = Width $evals[5]; $hidden = Width $evals[6]; $peek = Width $evals[7]; $open = Width $evals[8]
Check ($mid -gt $open -and $mid -lt $hidden) "sidebar animation: page width is mid-way while hiding ($mid)"
Check (($hidden - $open) -gt 200 -and ($hidden - $open) -lt 260) "sidebar: hiding gives its width to the page ($hidden vs $open)"
Check ($open -gt 0 -and $open -lt $hidden) "sidebar: toggling again reopens it ($open)"
Check ($peek -eq $hidden) "sidebar flyout: floats over the page without pushing it ($peek)"
Check ($evals[9] -match 'open' -and $evals[9] -match 'pre|pop|intro') 'palette animation: opening plays the intro/pop classes'
Check ($evals[10] -match 'open') 'palette: open after animation'
Check ($evals[11] -notmatch 'open') 'palette: closes'
Check ($evals[12] -match 'open' -and $evals[12] -notmatch 'pre|pop|intro') 'animations off: palette opens without animation classes'
Check ($evals[13] -match 'false\|Update to v9\.9\.9') 'update button: shown with the new version'
Check ($evals[15] -match 'up-bounce') 'update button: hover animation runs'
Check ($evals[16] -match 'Updating to v9\.9\.9') 'update button: downloading state'
Check ($evals[17] -match 'true') 'update button: hidden when up to date'
Check ($evals[18] -match 'windows' -and $evals[18] -match 'maxed') 'window: maximize button state follows the window'
Check ($evals[19] -match 'windows' -and $evals[19] -notmatch 'maxed') 'window: restore clears the maximized state'
Check ($saved.Count -eq 17) "screenshots saved ($($saved.Count)/17)"
if ($fail) { Write-Host "evals:"; $evals }
exit [int]$fail
