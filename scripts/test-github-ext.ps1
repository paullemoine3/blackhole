# Add from GitHub on Windows (Settings > Extensions, market step 3): a wrong repository is told,
# a repository found returns its release and the package for this system (Hendrizzzz/ghostify is
# also in the market: the panel says so), the package is downloaded and checked against GitHub's
# digest, then installed, pinned, loaded and marked "not verified". Scratch profile %TEMP%\bh_data
# (wiped first); needs network access and GitHub's anonymous API (60 requests an hour per address).
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data') -ErrorAction SilentlyContinue

$script = @'
wait 9000
cmd {"cmd":"open_settings","tab":"extensions"}
wait 2500
eval settings (document.getElementById('mkgh').click(), document.querySelector('#mkaddmenu [data-add="github"]').click(), document.getElementById('mkgithub').hidden + '|' + !!document.getElementById('ghrepo'))
eval settings (document.getElementById('ghrepo').value = 'not a repo', ghLookup(), 'asked')
wait 1500
eval settings document.querySelector('#mkgithub .fielderr') ? 'error-shown' : 'none'
eval settings (mk.gh = true, send({ cmd: 'github_cancel' }), 'asked')
wait 800
eval settings (document.getElementById('ghrepo').value = 'https://github.com/Hendrizzzz/ghostify', ghLookup(), 'asked')
wait 12000
eval settings JSON.stringify(lastState.github.flow && { stage: lastState.github.flow.stage, inMarket: lastState.github.flow.inMarket, n: (lastState.github.flow.assets || []).length, rec: (lastState.github.flow.assets || []).filter(a => a.recommended).length })
shots gh-choose
eval settings (document.querySelector('#mkgithub .btn.primary:not(.small)').click(), 'asked')
wait 25000
eval settings JSON.stringify(lastState.github.flow && { stage: lastState.github.flow.stage, sha: (lastState.github.flow.sha256 || '').length, digest: lastState.github.flow.digest, perms: (lastState.github.flow.permissions || []).length })
shots gh-ready
eval settings (send({ cmd: 'github_install' }), 'asked')
wait 6000
eval settings JSON.stringify(lastState.github.flow && { stage: lastState.github.flow.stage, id: lastState.github.flow.id })
eval settings (ghClose(), 'closed')
wait 1000
eval settings JSON.stringify(lastState.catalog.filter(c => c.unverified).map(c => c.id + ':' + c.installed + ':' + c.status))
eval settings (document.querySelector('#catalog .xc[data-id^="gh-"]').click(), document.querySelectorAll('#mkdetail .tag.warn').length + '|' + document.querySelectorAll('#mkdetail .panel.warn').length)
shots gh-sheet
quit
'@
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii

$logFile = Join-Path $tmp 'bh_run.log'
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(400000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
$log = Get-Content $logFile
$evals = @($log | Where-Object { $_ -match '\[dev\] eval ->' })

$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
Check ($evals[0] -match 'false\|true') 'panel: Add from GitHub opens with its field'
Check ($evals[2] -match 'error-shown') 'lookup: something that is not a repository is told'
Check ($evals[5] -match 'stage.{1,6}choose' -and $evals[5] -match 'inMarket.{1,6}ghostify' -and $evals[5] -match 'rec.{1,3}1') 'lookup: the release, its package for this system, and the market entry of the same repository'
Check ($evals[7] -match 'stage.{1,6}ready' -and $evals[7] -match 'sha.{1,3}64' -and $evals[7] -match 'digest.{1,3}true' -and $evals[7] -notmatch 'perms.{1,3}0[,}]') 'prepare: hash checked against GitHub, permissions shown, nothing installed yet'
Check ($evals[9] -match 'stage.{1,6}done' -and $evals[9] -match 'gh-hendrizzzz-ghostify') 'install: pinned and installed'
Check ($evals[11] -match 'gh-hendrizzzz-ghostify:true:(active|loading)') 'market: the extension is loaded and listed as not verified'
Check ($evals[12] -match '-> "[1-9]\|[1-9]') 'sheet: marked not verified, with its warning'
if ($fail) { exit 1 }
