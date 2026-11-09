# Pairing, tab sync and "Continue on" between BlackHole for Windows (devtools build) and
# BlackHole for Android on the emulator. Run scripts\test-android.ps1 first (it installs the APK
# and boots nothing else). Uses the scratch profile %TEMP%\bh_data (wiped) and clears the app's
# data on the emulator; screenshots go to %TEMP%\bh_shots (desktop) and %TEMP%\bh_android_shots.
#
# The emulator cannot see the local network's broadcasts, so the two talk directly:
#   desktop  listens on 127.0.0.1:47808, sends to 127.0.0.1:47809
#   `adb emu redir` forwards the host's UDP 47809 to the emulator's 47807
#   Android  listens on 47807, sends to 10.0.2.2:47808 (the host's loopback)
$ErrorActionPreference = 'Continue'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { 'E:\Android\sdk' }
$adb = Join-Path $sdk 'platform-tools\adb.exe'
$pkg = 'app.blackhole.browser'
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue

function B64($s) { [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($s)) }
function ACmd($json) { & $adb shell am start -n "$pkg/.MainActivity" --es bh_cmd64 (B64 $json) | Out-Null }
function ARun($js) { & $adb shell am start -n "$pkg/.MainActivity" --es bh_eval64 (B64 $js) | Out-Null }
function AEval($name, $js) { ARun "console.log('bheval:${name}=' + JSON.stringify((() => { try { return ($js); } catch (e) { return 'error: ' + e; } })()))" }
function AValue($name) {
    $line = & $adb logcat -d -s BlackHole:I | Select-String "bheval:${name}=" | Select-Object -Last 1
    if ($line -and "$line" -match "bheval:${name}=(.*)$") { $Matches[1] } else { $null }
}
# Polls an Android value until it matches.
function AWait($name, $js, $pattern, $seconds = 30) {
    $until = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $until) {
        AEval $name $js
        Start-Sleep -Milliseconds 900
        $v = AValue $name
        if ($v -and $v -match $pattern) { return $v }
    }
    return AValue $name
}
function AShot($name) {
    $dir = Join-Path $tmp 'bh_android_shots'; New-Item -ItemType Directory -Force $dir | Out-Null
    & $adb shell screencap -p "/sdcard/$name.png" | Out-Null
    & $adb pull "/sdcard/$name.png" (Join-Path $dir "$name.png") 2>&1 | Out-Null
}

# ---- Android side ---------------------------------------------------------------------------
& $adb wait-for-device | Out-Null
& $adb emu redir del udp:47809 2>&1 | Out-Null
& $adb emu redir add udp:47809:47807 | Out-Null
& $adb shell pm clear $pkg | Out-Null
& $adb logcat -c
& $adb shell am start -W -n "$pkg/.MainActivity" --es bh_peer '10.0.2.2:47808' | Out-Null
Start-Sleep -Seconds 4
ACmd '{"cmd":"navigate","input":"https://phone-tab.example/","new_tab":true}'

# ---- desktop side (script of the devtools harness) -----------------------------------------
$script = @'
wait 3000
cmd {"cmd":"open","input":"https://desk-tab.example/"}
wait 1500
cmd {"cmd":"open_settings","tab":"devices"}
wait 1500
cmd {"cmd":"device_show_code"}
wait 1500
eval settings 'code:' + document.querySelector('#devcodetext').textContent
wait 40000
shots 40-sync-devices
eval chrome 'tabs:' + state.tabs.map(t => t.url).join('|')
eval settings 'mode:' + [...document.querySelectorAll('#devlist .seg button.on')].map(b => b.textContent).join('|')
shot 41-sync-sidebar
quit
'@
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii
$logFile = Join-Path $tmp 'bh_sync.log'
$env:BH_HANDOFF_PORT = '47808'
$env:BH_HANDOFF_PEER_PORT = '47809'
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
Remove-Item Env:BH_HANDOFF_PORT, Env:BH_HANDOFF_PEER_PORT

# The desktop shows a code: the phone types it.
$code = $null
$until = (Get-Date).AddMinutes(6)
while (-not $code -and (Get-Date) -lt $until -and -not $proc.HasExited) {
    Start-Sleep -Seconds 1
    $hit = Get-Content $logFile -ErrorAction SilentlyContinue | Select-String '-> "code:([0-9A-Z-]+)"' | Select-Object -First 1
    if ($hit -and "$hit" -match 'code:([0-9A-Z-]+)') { $code = $Matches[1] }
}
if ($code) { ACmd ('{"cmd":"device_join","code":"' + $code + '"}') }

# Paired: the phone proposes to merge the desktop's tabs; "Always" merges and answers.
$offer = AWait 'offer' "document.querySelector('#pop').hidden ? '' : document.querySelector('#pop').textContent" 'Synchronize tabs with' 30
AShot '30-sync-offer'
ARun "[...document.querySelectorAll('#pop .act')].find((b) => b.textContent.startsWith('Always')).click()"
$phoneTabs = AWait 'phonetabs' "S.tabs.map((t) => t.url).join('|')" 'desk-tab\.example' 20
AShot '31-sync-merged'

# Continue on: a tab of the phone opens on the desktop.
ACmd '{"cmd":"navigate","input":"https://continued.example/read","new_tab":true}'
Start-Sleep -Seconds 2
ARun "send({ cmd: 'continue_on', device: S.devices.list[0].id })"
$sent = AWait 'sent' "document.querySelector('#toast-t').textContent" 'Opened on' 15
$mode = AWait 'mode' "S.devices.list[0] && S.devices.list[0].sync" 'auto' 10

if (-not $proc.WaitForExit(120000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL desktop timeout' }
& $adb emu redir del udp:47809 2>&1 | Out-Null
$log = Get-Content $logFile
$tabs = $log | Select-String '-> "tabs:' | Select-Object -First 1
$dmode = $log | Select-String '-> "mode:' | Select-Object -First 1

$failed = 0
function Check($ok, $name) {
    if ($ok) { Write-Host "PASS $name" -ForegroundColor Green } else { Write-Host "FAIL $name" -ForegroundColor Red; $script:failed++ }
}
Check ([bool]$code) "the desktop shows a pairing code ($code)"
Check ("$offer" -match 'Synchronize tabs with') 'the phone proposes to merge the desktop tabs'
Check ("$phoneTabs" -match 'desk-tab\.example' -and "$phoneTabs" -match 'phone-tab\.example') 'the phone has both tabs after Always'
Check ("$tabs" -match 'phone-tab\.example') 'the desktop got the phone tab back (answer applied without a question)'
Check ("$tabs" -match 'continued\.example') 'Continue on: the phone tab opened on the desktop'
Check ("$sent" -match 'Opened on') 'the phone heard that the tab opened'
Check ("$mode" -match 'auto') 'Always set the device to automatic sync'
Write-Host "desktop: $tabs"
Write-Host "desktop mode: $dmode"
if ($failed) { Write-Host "$failed check(s) failed" -ForegroundColor Red; exit 1 }
Write-Host 'All checks passed' -ForegroundColor Green
