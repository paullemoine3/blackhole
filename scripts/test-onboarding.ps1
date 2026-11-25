# Needs the network (Dark Reader and SponsorBlock are downloaded from their releases).
# Welcome tour (0.8.23) on Windows, driven by the devtools harness (see crates/app/src/devtools.rs):
# at the first launch the start page is veiled once the splash has landed and a dialog walks
# through the features (the hole between two steps, mid-transition shot), the import, the default
# browser, the extensions (shown, nothing installed: no network), the style (the space's color
# changes live) and the end, which lifts the veil of the sidebar and top bar. Then animations
# off (steps swap at once), Escape, and the features replayed from Settings > General.
# Uses the scratch profile %TEMP%\bh_data (wiped first: a first launch); screenshots land in
# %TEMP%\bh_shots. BH_ONBOARDING=1 lets the tour show in a devtools build.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue

$script = @'
wait 600
cmd {"cmd":"set_language","language":"en"}
wait 900
shot ob00-splash
wait 2600
eval home 'ob1:' + (!document.getElementById('ob').hidden) + '|' + document.getElementById('ob-title').textContent + '|' + document.querySelectorAll('#ob-prog i').length
eval chrome 'ob2:' + document.documentElement.classList.contains('bh-dimmed')
eval bar 'ob3:' + document.documentElement.classList.contains('bh-dimmed') + '|' + getComputedStyle(document.querySelector('.win')).zIndex
shot ob01-welcome
eval home 'ob4:' + (document.getElementById('ob-next').click(), 'next')
wait 380
eval home 'ob5:' + document.getElementById('ob-warp').classList.contains('go')
shot ob02-hole
wait 1000
eval home 'ob6:' + document.getElementById('ob-title').textContent
shot ob03-spaces
eval home 'ob7:' + (document.getElementById('ob-next').click(), 'next')
wait 1200
shot ob04-command
eval home 'ob8:' + (document.getElementById('ob-next').click(), 'next')
wait 1600
eval home 'ob9:' + document.getElementById('ob-title').textContent
shot ob05-shields
eval home 'ob50:' + (document.getElementById('ob-next').click(), 'next')
wait 2900
shot ob05b-pulsars-signal
wait 2600
eval home 'ob51:' + document.getElementById('ob-title').textContent + '|' + document.querySelectorAll('.ob-pf li.show').length + '|' + document.querySelector('.ob-pf .hd em').textContent
shot ob05c-pulsars
eval home 'ob52:' + (document.querySelector('.ob-step').scrollHeight <= document.querySelector('.ob-stage').clientHeight)
eval home 'ob10:' + (document.getElementById('ob-skip').click(), 'skip')
wait 1200
eval home 'ob11:' + document.getElementById('ob-title').textContent + '|' + document.getElementById('ob-next').textContent
shot ob06-import
eval home 'ob12:' + (document.getElementById('ob-skip').click(), 'later')
wait 1200
eval home 'ob13:' + document.getElementById('ob-title').textContent + '|' + document.getElementById('ob-next').textContent + '|' + document.querySelector('.ob-sys').textContent
shot ob07-default
eval home 'ob14:' + (document.getElementById('ob-skip').click(), 'not now')
wait 1200
eval home 'ob15:' + document.getElementById('ob-title').textContent + '|' + document.querySelectorAll('.ob-ext').length + '|' + document.getElementById('ob-next').textContent
shot ob08-extensions
eval home 'ob39:' + (window.__busy = [], new MutationObserver(() => { const b = document.querySelectorAll('.ob-ext .st.busy').length; if (b) __busy.push(b + '|' + document.getElementById('ob-next').textContent); }).observe(document.getElementById('ob-stage'), { subtree: true, childList: true }), 'watching')
eval home 'ob40:' + (document.getElementById('ob-next').click(), 'install')
wait 600
eval home 'ob41:' + (__busy[0] || 'never')
shot ob08b-downloading
wait 30000
eval home 'ob42:' + document.getElementById('ob-next').textContent + '|' + [...document.querySelectorAll('.ob-ext .st')].map(x => x.textContent).join(',')
eval bar 'ob43:' + document.getElementById('url').textContent
eval chrome 'ob44:' + document.querySelectorAll('[data-id]').length
shot ob08c-installed
eval home 'ob16:' + (document.getElementById('ob-next').click(), 'next')
wait 1200
eval home 'ob17:' + document.getElementById('ob-title').textContent
eval home 'ob18:' + (document.querySelectorAll('.ob-sw8 button')[2].click(), 'green')
wait 700
eval home 'ob19:' + document.getElementById('ob').style.getPropertyValue('--ob-accent') + '|' + document.querySelectorAll('.ob-sw8 button')[2].getAttribute('aria-checked')
shot ob09-style
eval home 'ob20:' + (document.getElementById('ob-next').click(), 'next')
wait 1200
eval home 'ob21:' + document.getElementById('ob-title').textContent + '|' + document.querySelectorAll('.ob-recap div').length + '|' + (document.querySelector('.ob-step').scrollHeight <= document.querySelector('.ob-stage').clientHeight)
shot ob10-ready
eval home 'ob22:' + (document.getElementById('ob-next').click(), 'go')
wait 450
shot ob11-falling
wait 1200
eval home 'ob23:' + document.getElementById('ob').hidden + '|' + document.getElementById('ob-veil').classList.contains('on')
eval chrome 'ob24:' + document.documentElement.classList.contains('bh-dimmed')
eval bar 'ob25:' + document.documentElement.classList.contains('bh-dimmed')
shot ob12-home
cmd {"cmd":"set_animations","on":false}
cmd {"cmd":"replay_tour"}
wait 800
eval home 'ob26:' + (!document.getElementById('ob').hidden) + '|' + document.querySelectorAll('#ob-prog i').length + '|' + document.getElementById('ob-skip').textContent
eval chrome 'ob27:' + document.documentElement.classList.contains('bh-dimmed')
eval home 'ob28:' + (document.getElementById('ob-next').click(), document.getElementById('ob-title').textContent)
shot ob13-tour-still
eval home 'ob53:' + (document.getElementById('ob-next').click(), document.getElementById('ob-next').click(), document.getElementById('ob-next').click(), document.getElementById('ob-title').textContent) + '|' + document.querySelectorAll('.ob-pf li.show').length
wait 300
shot ob13b-pulsars-still
eval home 'ob29:' + (document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })), document.getElementById('ob').hidden)
wait 300
eval chrome 'ob30:' + document.documentElement.classList.contains('bh-dimmed')
cmd {"cmd":"set_animations","on":true}
cmd {"cmd":"set_language","language":"fr"}
cmd {"cmd":"replay_tour"}
wait 900
eval home 'ob31:' + document.getElementById('ob-title').textContent
eval home 'ob32:' + (document.getElementById('ob-next').click(), document.getElementById('ob-next').click(), 'twice')
wait 3000
eval home 'ob33:' + document.getElementById('ob-title').textContent
shot ob14-tour-command-fr
eval home 'ob34:' + (document.getElementById('ob-next').click(), 'next')
wait 1200
eval home 'ob36:' + (document.getElementById('ob-next').click(), 'next')
wait 1200
eval home 'ob35:' + document.getElementById('ob-title').textContent
shot ob15-shields-fr
eval home 'ob54:' + (document.getElementById('ob-next').click(), 'next')
wait 5500
eval home 'ob55:' + (document.querySelector('.ob-step').scrollHeight <= document.querySelector('.ob-stage').clientHeight)
shot ob16-pulsars-fr
cmd {"cmd":"set_language","language":"en"}
quit
'@
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii

$logFile = Join-Path $tmp 'bh_run.log'
$env:BH_ONBOARDING = '1'
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(540000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
Remove-Item Env:BH_ONBOARDING
$log = Get-Content $logFile
$all = @($log | Where-Object { $_ -match '\[dev\] eval ->' })
function Ev($name) { [string]($all | Where-Object { $_ -match "-> `"${name}:" } | Select-Object -First 1) }
$log | Where-Object { $_ -match '\[dev\] (bad|unknown|eval failed|screenshot failed)' } | ForEach-Object { Write-Host $_ }

$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
Check ((Ev ob1) -match 'ob1:true\|Welcome to BlackHole\|(10|11)"') 'first launch: the dialog opens over the start page after the splash'
Check ((Ev ob2) -match 'ob2:true"') 'the sidebar is veiled under the dialog'
Check ((Ev ob3) -match 'ob3:true\|2147483001"') 'the top bar is veiled, its window buttons stay above the veil'
Check ((Ev ob5) -match 'ob5:true"') 'between two steps the hole swallows the card (mid-transition)'
Check ((Ev ob6) -match 'ob6:A space for every context"') 'the hole brings the next step'
Check ((Ev ob9) -match 'ob9:Not even light gets out"') 'the Shields step has its new title'
Check ((Ev ob51) -match 'ob51:Folders that fill themselves\|[123]\|[123]"') 'the Pulsars step: the folder fills, its count follows'
Check ((Ev ob52) -match 'ob52:true') 'it fits without scrolling'
Check ((Ev ob53) -match 'ob53:Folders that fill themselves\|3') 'animations off: the folder is shown filled'
Check ((Ev ob55) -match 'ob55:true') 'it fits in French too'
Check ((Ev ob11) -match 'ob11:Bring your bookmarks\|') 'Skip the tour goes to the import'
Check ((Ev ob13) -match 'ob13:Open your links in BlackHole\|') 'then the default browser'
Check ((Ev ob13) -match 'installer|Click here to open Settings') 'the Windows text points to Settings (or to the installer for a cargo build)'
Check ((Ev ob15) -match 'ob15:A few extensions to get started\|[1-4]\|Install 2 extensions') 'the starter extensions of the market, two ticked'
Check ((Ev ob41) -match 'ob41:[12]\|Installing') 'installing: the extensions said they were downloading, the button waited'
Check ((Ev ob42) -match 'ob42:Continue.+\|.+Installed,.+Installed"') 'installed: both marked, the button goes on'
Check (((Ev ob43) -match 'ob43:') -and ((Ev ob43) -notmatch 'chrome-extension|darkreader|sponsor|github')) 'no welcome page of an extension opened over the tour'
Check ((Ev ob17) -match 'ob17:Make it yours"') 'then the style'
Check ((Ev ob19) -match 'ob19:#2fd4a0\|true"') 'a color picked recolors the space and the dialog'
Check ((Ev ob21) -match 'ob21:Have a good trip to the edge of the horizon\|4\|true"') 'the last step fits without scrolling'
Check ((Ev ob23) -match 'ob23:true\|false"') 'the end lets the start page come out of the hole'
Check ((Ev ob24) -match 'ob24:false"') 'the sidebar veil is lifted'
Check ((Ev ob25) -match 'ob25:false"') 'the top bar veil is lifted'
Check ((Ev ob26) -match 'ob26:true\|6\|Close"') 'Settings replays the features only (six steps)'
Check ((Ev ob27) -match 'ob27:true"') 'the replay veils the sidebar too'
Check ((Ev ob28) -match 'ob28:A space for every context"') 'animations off: the next step comes at once'
Check ((Ev ob29) -match 'ob29:true"') 'Escape closes the tour'
Check ((Ev ob30) -match 'ob30:false"') 'and lifts the veil'
Check ((Ev ob31) -match 'ob31:Bienvenue dans BlackHole"') 'the tour follows the interface language'
Check ((Ev ob33) -match 'ob33:Un espace pour chaque contexte"') 'a click while the hole runs is ignored (one step at a time)'
Check ((Ev ob35) -match 'ob35:M.+me la lumi.+re n.+en ressort pas"') 'the Shields title in French'
Write-Host "Screenshots: $(Join-Path $tmp 'bh_shots')"
if ($fail) { exit 1 }
