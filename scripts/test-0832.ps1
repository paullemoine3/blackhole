# 0.8.32 on Windows, driven by the devtools harness: the Shields' counter with uBlock Origin Lite.
# The extension stops a request before WebView2 asks the Shields; a tracker of the Shields' lists is
# counted all the same, once, and what only the extension refuses is not.
# Uses the scratch profile %TEMP%\bh_data (wiped first); needs network access (uBlock, example.com).
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data') -ErrorAction SilentlyContinue
# A listed host that loads from a tag at once (before the load event), one late, one unlisted.
$ask = "(['https://www.googletagmanager.com/gtm.js?id=GTM-1','https://pbs.optidigital.com/x.js','https://cdn.by.wonderpush.com/sdk/1.1/wonderpush-loader.min.js'].forEach(u => { const s = document.createElement('script'); s.src = u; document.head.append(s); }), 'asked')"
$script = @'
wait 3000
cmd {"cmd":"open","input":"https://example.com"}
wait 4000
eval content __ASK__
wait 12000
eval chrome 'a:' + state.blocked
cmd {"cmd":"install_catalog","id":"ublock-origin-lite"}
wait 25000
cmd {"cmd":"open","input":"https://example.org"}
wait 4000
eval content __ASK__
wait 12000
eval chrome 'b:' + state.blocked
cmd {"cmd":"open","input":"https://www.01net.com"}
wait 16000
eval chrome 'c:' + state.blocked
cmd {"cmd":"open_library","section":"shields"}
wait 2500
eval library 'd:' + [...document.querySelectorAll('.shrow')].map(r => r.textContent).join(',')
shot s0832-shields
quit
'@
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value ($script -replace '__ASK__', $ask) -Encoding ascii
$logFile = Join-Path $tmp 'bh_run.log'
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(400000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
$all = @(Get-Content $logFile -Encoding utf8 | Where-Object { $_ -match '\[dev\] (eval|bad|unknown)' })
$all | ForEach-Object { Write-Host "  $_" }
function Num($name) { if (($all -join "`n") -match "`"${name}:(\d+)") { [int]$Matches[1] } else { -1 } }
$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
Check ((Num a) -eq 2) 'without an extension: the two listed hosts, once each'
Check ((Num b) -eq 4) 'with uBlock: the same two again, not the host only uBlock refuses'
if ($fail) { exit 1 }
