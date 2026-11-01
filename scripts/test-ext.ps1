# Extension scenario on Windows (WebView2): installs Dark Reader from the catalog, checks it is
# injected into a plain page and that its toolbar button is present. Uses the scratch profile
# %TEMP%\bh_data (wiped first); needs network access.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data') -ErrorAction SilentlyContinue

# info.cern.ch is a plain light page (example.com already supports dark mode, so Dark Reader skips it).
$script = @'
wait 3000
cmd {"cmd":"install_catalog","id":"dark-reader"}
wait 15000
eval bar document.querySelectorAll('#exts button').length
cmd {"cmd":"open","input":"https://info.cern.ch"}
wait 12000
eval content (document.documentElement.outerHTML.match(/darkreader/gi) || []).length + '|' + getComputedStyle(document.body).backgroundColor
quit
'@
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii

$logFile = Join-Path $tmp 'bh_run.log'
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(240000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
$log = Get-Content $logFile
$evals = @($log | Where-Object { $_ -match '\[dev\] eval ->' })

$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
Check ($evals[0] -match '[1-9]') 'toolbar: extension button present'
Check ($evals[1] -match 'rgb\(24, 26, 27\)') 'content: Dark Reader darkens the page'
if ($fail) { exit 1 }
