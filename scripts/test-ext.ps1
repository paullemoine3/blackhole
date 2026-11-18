# Extension scenario on Windows (WebView2): installs Dark Reader from the market, checks it is
# injected into a plain page and that its toolbar button is present; then uBlock Origin Lite,
# whose popup must see the page in front as its active tab (WebView2 answers with the popup). Uses the scratch profile
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
eval bar [...document.querySelectorAll('#exts button')].map(b => b.title).join('|')
eval chrome state.tabs.length
eval content (window.open('chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/popup/index.html?uilocation=popout', '_blank', 'width=380,height=630'), 'asked')
wait 2500
eval chrome state.tabs.length
shot ext-popout
cmd {"cmd":"close_ext_popup"}
cmd {"cmd":"install_catalog","id":"ublock-origin-lite"}
wait 15000
cmd {"cmd":"open","input":"https://example.com"}
wait 8000
cmd {"cmd":"ext_action","id":"ublock-origin-lite","x":900,"y":0,"w":30,"h":38}
wait 4000
eval extpopup (window.__front = '', chrome.tabs.query({active:true,currentWindow:true}).then(t => __front = t.map(x => x.url).join('|')), 'asked')
wait 1000
eval extpopup window.__front + '#' + document.body.innerText.replace(/\s+/g, ' ').slice(0, 120)
shot ext-ubol
cmd {"cmd":"close_ext_popup"}
cmd {"cmd":"open_settings","tab":"extensions"}
wait 1500
cmd {"cmd":"check_catalog_updates"}
wait 8000
eval settings document.querySelectorAll('#catalog .row').length + '|' + document.getElementById('catsrc').textContent + '|' + document.getElementById('chk-msg').textContent
shot ext-market
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
Check ($evals[2] -match 'Dark Reader' -and $evals[2] -notmatch '__MSG_') 'toolbar: the button carries the name of the extension'
Check ($evals[3] -match '\d' -and ($evals[3] -replace '.*-> ', '') -eq ($evals[5] -replace '.*-> ', '')) 'popout: a window asked by an extension from a page opens as its popup, not as a tab'
Check ($evals[7] -match 'https://example\.com/#' -and $evals[7] -notmatch 'pas un site|not a web') 'popup: the active tab of an extension popup is the page in front'
# Market: six entries for this system; before the first signed list is published, the build's own
# list stays and the note says so (with the failed download), the check still answers.
Check ($evals[8] -match '-> "?6\|' -and $evals[8] -match 'included in this version|incluse dans cette version' -and $evals[8] -match 'up to date|update|à jour|mise')'settings: the market lists the extensions of this system and says where the list comes from'
if ($fail) { exit 1 }
