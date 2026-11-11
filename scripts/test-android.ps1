# UI scenarios of BlackHole for Android on an emulator or a USB device (adb).
# Installs the debug APK, wipes the app's data, drives the interface through debug-only
# intent extras (bh_cmd64: an interface message, bh_eval64: JavaScript in the interface page),
# reads the `bheval:` results from logcat and takes screenshots into %TEMP%\bh_android_shots.
# Needs network access (example.com…). Usage:
#   scripts\test-android.ps1 [-Apk path] [-Serial emulator-5554] [-Boot]   (-Boot starts the AVD bh_test)
param(
    [string]$Apk = 'android\app\build\outputs\apk\debug\app-debug.apk',
    [string]$Serial = '',
    [switch]$Boot
)
# adb writes progress to stderr: PowerShell 5.1 must not turn it into a terminating error.
$ErrorActionPreference = 'Continue'
Set-Location (Split-Path $PSScriptRoot -Parent)

$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } elseif (Test-Path 'E:\Android\sdk') { 'E:\Android\sdk' } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
$adb = Join-Path $sdk 'platform-tools\adb.exe'
if (-not (Test-Path $adb)) { throw "adb not found under $sdk (set ANDROID_HOME)" }
$pkg = 'app.blackhole.browser'
$shots = Join-Path ([IO.Path]::GetTempPath()) 'bh_android_shots'
Remove-Item -Recurse -Force $shots -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $shots | Out-Null

function Adb { if ($Serial) { & $adb -s $Serial @args } else { & $adb @args } }

if ($Boot -and -not ((Adb devices) -match 'device$')) {
    $emu = Join-Path $sdk 'emulator\emulator.exe'
    Start-Process -FilePath $emu -ArgumentList '-avd', 'bh_test', '-no-boot-anim', '-no-audio', '-no-snapshot-save' | Out-Null
}
Adb wait-for-device | Out-Null
for ($i = 0; $i -lt 120 -and (Adb shell getprop sys.boot_completed 2>$null) -notmatch '1'; $i++) { Start-Sleep -Seconds 2 }

Adb install -r -t $Apk | Out-Null
Adb shell pm clear $pkg | Out-Null
Adb logcat -c
Adb shell am start -W -n "$pkg/.MainActivity" | Out-Null

# Local test pages, reached from the device through `adb reverse` (loopback: plain http stays
# plain, and no HTTPS inspection on this computer gets in the way). Page A asks for a tracker.
$port = 18420
Adb reverse "tcp:$port" "tcp:$port" | Out-Null
$base = "http://127.0.0.1:$port"
$server = Start-Job -ArgumentList $port -ScriptBlock {
    param($port)
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $port)
    $listener.Start()
    $deadline = (Get-Date).AddMinutes(8)
    while ((Get-Date) -lt $deadline) {
        if (-not $listener.Pending()) { Start-Sleep -Milliseconds 30; continue }
        $client = $listener.AcceptTcpClient()
        try {
            $stream = $client.GetStream()
            $reader = [IO.StreamReader]::new($stream, [Text.Encoding]::ASCII, $false, 4096, $true)
            $path = (($reader.ReadLine()) -split ' ')[1]
            while ($reader.ReadLine()) {}
            $status = '200 OK'
            # A 1x1 PNG as the page icon: the shell hands it to the core (favicons).
            $icon = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=='
            $page = { param($t, $c) "<!doctype html><meta name=viewport content='width=device-width'><link rel=icon href='$icon'><title>$t</title><body style='font:18px sans-serif;margin:24px;background:$c'><h1>$t</h1><p>BlackHole Android test page.</p>" }
            if ($path -like '/a*') { $body = (& $page 'Alpha page' '#fdf3e7') + "<img src='https://stats.g.doubleclick.net/pixel.gif' width=1 height=1><p>" + ('Lorem ipsum dolor sit amet. ' * 40) }
            elseif ($path -like '/b*') { $body = & $page 'Beta page' '#e8f4fd' }
            elseif ($path -like '/p*') { $body = & $page 'Private page' '#f1e8fd' }
            else { $status = '404 Not Found'; $body = '' }
            $bytes = [Text.Encoding]::UTF8.GetBytes($body)
            $head = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 $status`r`nContent-Type: text/html; charset=utf-8`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n")
            $stream.Write($head, 0, $head.Length); $stream.Write($bytes, 0, $bytes.Length); $stream.Flush()
        } catch {} finally { $client.Close() }
    }
    $listener.Stop()
}

function B64($s) { [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($s)) }
# An interface message (`MobileCommand`, or a shell-local one such as `surface`).
function Cmd($json) { Adb shell am start -n "$pkg/.MainActivity" --es bh_cmd64 (B64 $json) | Out-Null }
# JavaScript in the interface page; `Eval name js` logs `bheval:name=<JSON of the value>`.
function Run($js) { Adb shell am start -n "$pkg/.MainActivity" --es bh_eval64 (B64 $js) | Out-Null }
function Eval($name, $js) { Run "console.log('bheval:${name}=' + JSON.stringify((() => { try { return ($js); } catch (e) { return 'error: ' + e; } })()))" }
function Wait($ms) { Start-Sleep -Milliseconds $ms }
function Shot($name) {
    Adb shell screencap -p "/sdcard/$name.png" | Out-Null
    Adb pull "/sdcard/$name.png" (Join-Path $shots "$name.png") 2>&1 | Out-Null
    Adb shell rm "/sdcard/$name.png" | Out-Null
}
function Key($code) { Adb shell input keyevent $code | Out-Null }

# ---- scenario ----------------------------------------------------------------------------
Wait 4000
Shot '01-home'
Eval home "!document.querySelector('#home').hidden && S && S.current === null"
Cmd ('{"cmd":"navigate","input":"' + $base + '/a","new_tab":true}')
Wait 5000
Shot '02-page'
Eval page "S.url + ' ' + S.title"
Eval blocked "S.blocked.site"
Eval favicon "icons.size + ' ' + !document.querySelector('#site').hidden"
# Find in page (the bar's row becomes a search field).
Run "startFind(); document.querySelector('#fq').value = 'Lorem'; document.querySelector('#fq').dispatchEvent(new Event('input'))"
Wait 1500
Shot '19-find'
Eval find "document.querySelector('#fcount').textContent"
Run "endFind()"
Wait 300
# Desktop site, then back to the mobile one.
Run "send({ cmd: 'desktop_site', tab: S.current })"
Wait 1500
Eval desktop "S.desktop"
Run "send({ cmd: 'desktop_site', tab: S.current })"
Wait 1200
Cmd ('{"cmd":"navigate","input":"' + $base + '/b"}')
Wait 4000
Eval second "S.url + ' ' + S.tabs.length"
# A real tap on the bar, real typing: our keyboard covers the bar (it does not climb, so the
# page under it keeps its size) and the address field keeps what is typed.
Adb shell settings put secure show_ime_with_hard_keyboard 1 | Out-Null
$wm = "$(Adb shell wm size)"
if ($wm -match '(\d+)x(\d+)\s*$') { $sw = [int]$Matches[1]; $sh = [int]$Matches[2] } else { $sw = 1080; $sh = 2400 }
Eval barbefore "Math.round(document.querySelector('#bar').getBoundingClientRect().height)"
Adb shell input tap ([int]($sw / 2)) ($sh - 200) | Out-Null
Wait 1800
Adb shell input text 'alp' | Out-Null
Wait 1200
Shot '22-typing'
Eval typing "open.sheet + ' ' + document.querySelector('#q').value + ' ' + (kb > 0) + ' ' + kbOurs"
Eval barafter "Math.round(document.querySelector('#bar').getBoundingClientRect().height)"
Run "close('sheet')"
Wait 900
Run "openSheet(true, true)"
Wait 900
Shot '03-palette'
Eval sheet "document.querySelector('#sheet').classList.contains('on')"
Eval editurl "document.querySelector('#q').value"
Run "document.querySelector('#q').value='page'; document.querySelector('#q').dispatchEvent(new Event('input'))"
Wait 900
Shot '04-suggest'
Eval suggest "document.querySelectorAll('#list .act').length"
# Typing on: the rows still suggested are the same nodes, and no entry animation plays again.
Run "window.__rows = [...document.querySelectorAll('#list .act')]; document.querySelector('#q').value='pag'; document.querySelector('#q').dispatchEvent(new Event('input'))"
Wait 700
Eval keeprows "window.__rows.filter((n) => n.isConnected).length + ' ' + document.querySelectorAll('#list .act.in').length"
# Intermediate frame: the page freezes the drawer's transition 150 ms in.
Run "close('sheet'); setOpen('drawer', true); setTimeout(() => document.getAnimations().forEach((a) => a.pause()), 150)"
Wait 1500
Shot '05-drawer-moving'
Eval moving "getComputedStyle(document.querySelector('#drawer')).transform"
Run "document.getAnimations().forEach((a) => a.play())"
Wait 900
Shot '06-drawer'
Eval drawer "getComputedStyle(document.querySelector('#drawer')).transform"
Run "close('drawer'); setOpen('grid', true)"
Wait 1500
Shot '07-grid'
Eval grid "document.querySelectorAll('#cards .tc').length"
Run "close('grid')"
Wait 500
Cmd '{"cmd":"privacy_report"}'
Wait 900
Shot '08-report'
Eval report "!document.querySelector('#pop').hidden"
Run "close('pop')"
Cmd ('{"cmd":"navigate","input":"' + $base + '/p","new_tab":true,"private":true}')
Wait 4000
Shot '09-private'
Eval private "S.private"
Cmd '{"cmd":"close_private"}'
Wait 1500
Eval privgone "S.private_tabs.length + ' ' + S.private"
Cmd '{"cmd":"navigate","input":"https://no-such-host.invalid/","new_tab":true}'
Wait 6000
Shot '10-error-not-found'
Eval error "S.error"
Cmd '{"cmd":"navigate","input":"https://stats.g.doubleclick.net/"}'
Wait 1200
Eval shields "document.querySelector('#toast-t').textContent"
Run "setOpen('settings', true)"
Wait 900
Shot '11-settings'
Cmd '{"cmd":"set_setting","name":"theme","value":"dark"}'
Wait 900
Shot '12-settings-dark'
Eval dark "document.documentElement.dataset.theme"
Cmd '{"cmd":"set_setting","name":"theme","value":"system"}'
Cmd '{"cmd":"set_setting","name":"text_zoom","value":130}'
Wait 500
Eval zoom "S.settings.text_zoom"
Cmd '{"cmd":"set_setting","name":"text_zoom","value":100}'
# Updates: the manual check reaches GitHub (network) and comes back with an answer.
Run "document.querySelector('#updates').scrollIntoView()"
Cmd '{"cmd":"check_update","manual":true}'
Wait 8000
Shot '20-updates'
Eval update "S.update.status"
Run "document.querySelector('#devices').scrollIntoView()"
Wait 700
Shot '16-devices'
Eval devices "!document.querySelector('#devices').hidden && S.devices.available"
Cmd '{"cmd":"device_show_code"}'
Wait 1500
Shot '17-pairing-code'
Eval code "/^[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}$/.test(S.devices.code || '')"
Cmd '{"cmd":"device_cancel_pairing"}'
Run "close('settings')"
Wait 500
Run "receive({ type: 'sync_offer', id: 'none', name: 'Desk', text: '2 tabs to open' })"
Wait 900
Shot '18-sync-offer'
Eval offer "document.querySelector('#pop').textContent.includes('Synchronize tabs with Desk')"
Run "close('pop')"
# A site asking for the camera: closing the question refuses it.
Run "receive({ type: 'permission', id: 999, host: 'meet.example', kinds: ['camera', 'microphone'] })"
Wait 800
Shot '21-permission'
Eval permission "document.querySelector('#pop').textContent.includes('meet.example wants to use your camera and microphone')"
Run "close('pop')"
Run "receive({ type: 'link_menu', url: null, image: 'https://example.org/a.png' })"
Wait 800
Eval imagemenu "document.querySelector('#pop').textContent.includes('Download image')"
Run "close('pop')"
Cmd '{"cmd":"set_setting","name":"animations","value":false}'
Wait 600
Run "setOpen('drawer', true)"
Wait 120
Shot '13-drawer-still'
Eval still "document.body.classList.contains('still') + ' ' + getComputedStyle(document.querySelector('#drawer')).transform"
Run "close('drawer')"
Cmd '{"cmd":"set_setting","name":"animations","value":true}'
Run "openHistory()"
Wait 1200
Shot '14-history'
Eval history "document.querySelectorAll('#hlist .row').length"
Key 'KEYCODE_BACK'
Wait 900
Eval backclose "open.history"
Eval before "S.tabs.length"
Run "send({ cmd: 'close', tab: S.current })"
Wait 900
Shot '15-closed-toast'
Eval undo "!document.querySelector('#toast-undo').hidden"
Cmd '{"cmd":"undo_close"}'
Wait 1500
Eval after "S.tabs.length"
# Closing a tab in the grid: its card leaves, the others are the same nodes (no rebuild).
Cmd ('{"cmd":"navigate","input":"' + $base + '/a","new_tab":true}')
Wait 3000
Run "setOpen('grid', true)"
Wait 1500
Run "window.__cards = [...document.querySelectorAll('#cards .tc')]; flyAway(window.__cards[0], 1)"
Wait 1500
Shot '23-grid-closed'
Eval gridkeep "window.__cards.length + ' ' + window.__cards.slice(1).every((n) => n.isConnected) + ' ' + window.__cards[0].isConnected + ' ' + document.querySelectorAll('#cards .tc.in').length"
# A state that changes nothing in a part leaves its nodes alone (drawer rows).
Run "window.__row = document.querySelector('#dtabs .trow'); send({ cmd: 'set_setting', name: 'pull_refresh', value: false })"
Wait 700
Eval samerow "window.__row === document.querySelector('#dtabs .trow') && S.settings.pull_refresh === false"
Run "close('grid')"
Cmd '{"cmd":"home"}'
Wait 800
Key 'KEYCODE_BACK'
Wait 1200
# Back on the start page sends the app to the background (no eval: an intent would bring it back).
$focus = (Adb shell dumpsys window) | Select-String 'mCurrentFocus' | Select-Object -First 1

# ---- checks --------------------------------------------------------------------------------
$log = Adb logcat -d -s BlackHole:I
$evals = @{}
foreach ($line in $log) { if ($line -match 'bheval:([a-z]+)=(.*)$') { $evals[$Matches[1]] = $Matches[2] } }
$failed = 0
function Check($ok, $name) {
    if ($ok) { Write-Host "PASS $name" -ForegroundColor Green } else { Write-Host "FAIL $name" -ForegroundColor Red; $script:failed++ }
}
function V($n) { if ($evals.ContainsKey($n)) { $evals[$n] } else { '<missing>' } }
Check ((V home) -eq 'true') 'start page shown at launch'
Check ((V page) -match '127\.0\.0\.1:\d+/a Alpha page') 'address opens a tab (title shown)'
Check ([int]((V blocked) -replace '\D', '0') -ge 1) 'Shields block the tracker of a page'
Check ((V second) -match '/b 1"$') 'address bar loads in the same tab'
Check ((V favicon) -match '^"[1-9]\d* true"$') 'page icon kept and shown in the bar'
Check ((V find) -match '^"1/\d+"$') 'find in page counts the matches'
Check ((V desktop) -eq 'true') 'desktop site on the tab'
Check ((V sheet) -eq 'true') 'palette sheet opens'
Check ((V editurl) -match '127\.0\.0\.1:\d+/b') 'tapping the bar puts its address in the field'
Check ([int]((V suggest) -replace '\D', '0') -ge 2) 'suggestions listed'
Check ((V typing) -eq '"true alp true true"') 'tapping the bar and typing: our keyboard, the field keeps the text'
Check ((V barafter) -eq (V barbefore) -and (V barbefore) -ne '<missing>') 'the bar does not climb over our keyboard (the page keeps its size)'
Check ((V keeprows) -match '^"[2-9] 0"$') 'typing keeps the suggestion rows (no rebuild, no animation again)'
Check ((V gridkeep) -match '^"[2-9] true false 0"$') 'closing a tab in the grid removes its card only'
Check ((V samerow) -eq 'true') 'an unrelated state leaves the drawer rows alone'
Check ((V moving) -match 'matrix\(1, 0, 0, 1, -\d') 'drawer caught mid-slide'
Check ((V drawer) -match 'none|matrix\(1, 0, 0, 1, 0, 0\)') 'drawer slides in'
Check ((V grid) -eq '1') 'tab grid shows the tab'
Check ((V report) -eq 'true') 'privacy report opens'
Check ((V private) -eq 'true') 'private tab'
Check ((V privgone) -eq '"0 false"') 'private tabs closed'
Check ((V error) -eq '"not_found"') 'error page for an unknown host'
Check ((V shields) -match 'Shields') 'typed tracker address refused'
Check ((V dark) -eq '"dark"') 'dark theme forced in Settings'
Check ((V zoom) -eq '130') 'text size setting'
Check ((V update) -match '^"(latest|available)"$') 'manual update check answers (network)'
Check ((V permission) -eq 'true') 'site permission question'
Check ((V imagemenu) -eq 'true') 'long press menu of an image'
Check ((V devices) -eq 'true') 'Settings > Devices available (Keystore key)'
Check ((V code) -eq 'true') 'a pairing code is shown'
Check ((V offer) -eq 'true') 'the sync proposal shows Merge / Always / Later'
Check ((V still) -match '^"true (none|matrix\(1, 0, 0, 1, 0, 0\))"$') 'animations off: drawer opens at once'
Check ([int]((V history) -replace '\D', '0') -ge 2) 'history lists visits'
Check ((V backclose) -eq 'false') 'system back closes the history page'
Check ((V undo) -eq 'true') 'closing offers undo'
Check ((V after) -eq (V before)) 'undo brings the tab back'
Check ("$focus" -notmatch $pkg) 'back on the start page leaves the app'

Stop-Job $server -ErrorAction SilentlyContinue; Remove-Job $server -Force -ErrorAction SilentlyContinue
Adb reverse --remove "tcp:$port" | Out-Null
Write-Host "Screenshots: $shots"
Get-ChildItem $shots | ForEach-Object { Write-Host "  $($_.Name)" }
if ($failed) { Write-Host "$failed check(s) failed" -ForegroundColor Red; exit 1 }
Write-Host 'All checks passed' -ForegroundColor Green
