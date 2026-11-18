# 0.8.18 scenarios on Windows, driven by the devtools harness (see crates/app/src/devtools.rs):
# the command bar search ranked by the shell (one row per page, no sign-in steps, accents and a
# typo forgiven, the grey completion, a choice learned, a tab of another space), the reload arrow (it turns, the
# button does not), the Settings in the page card ("Changes applied", Mod+W), the start page
# sections moved (keyboard and pointer), the silent second try of a cut connection (a broken hop
# of a redirect too, never a redirect loop), the jump list.
# Uses the scratch profile %TEMP%\bh_data (wiped first); screenshots land in %TEMP%\bh_shots.
# No network: a local server on 47984. Every eval is named ("sr1:", "rl2:"…) and read by name.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue
$port = 47984
$started = Get-Date

$script = @'
wait 3000
cmd {"cmd":"set_language","language":"en"}
# ---- search: one row per page, no sign-in steps ----
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/page"}
wait 2500
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/page/"}
wait 2000
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/page?b=2&a=1"}
wait 2000
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/page?a=1&b=2"}
wait 2000
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/login?next=/"}
wait 2000
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/accent"}
wait 2000
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/other"}
wait 2000
cmd {"cmd":"palette","mode":"new"}
wait 1200
eval palette 'sr0:' + (window.__t = (v) => { const q = document.getElementById('q'); q.value = v; q.setSelectionRange(v.length, v.length); q.dispatchEvent(new Event('input')); return v; }, window.__rows = () => [...document.querySelectorAll('#list .row .t')].map(x => x.textContent), 'ready')
eval palette 'sr00:' + __t('bh')
wait 400
eval palette 'sr1:' + (() => { const t = __rows(); return t.filter(x => x === 'bh page').length + '|' + t.filter(x => x === 'bh login').length + '|' + t.filter(x => x === 'bh other').length; })()
eval palette 'sr01:' + __t('other bh')
wait 400
eval palette 'sr2:' + __rows().join(',')
eval palette 'sr02:' + __t('bh zzz')
wait 400
eval palette 'sr3:' + document.querySelectorAll('#list .row').length
eval palette 'sr03:' + __t('reglages')
wait 400
eval palette 'sr4:' + __rows().slice(1, 2).join(',')
eval palette 'sr04:' + __t('otehr')
wait 400
eval palette 'sr5:' + __rows().slice(1, 2).join(',')
eval palette 'sr05:' + __t('127.0')
wait 400
eval palette 'sr6:' + !document.getElementById('ghost').hidden + '|' + document.getElementById('ghost').textContent + '|' + __rows()[0]
shot g00-search-complete
eval palette 'sr7:' + (document.getElementById('q').dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true })), document.getElementById('q').value)
eval palette 'sr07:' + __t('bh')
wait 400
eval palette 'sr8:' + __rows()[1]
eval palette 'sr08:' + (window.ipc.postMessage(JSON.stringify({ cmd: 'suggest_chosen', input: 'bh', key: '127.0.0.1:__PORT__/accent' })), __t('bh p'), __t('bh'))
wait 400
eval palette 'sr9:' + __rows()[1]
shot g00-search
cmd {"cmd":"palette_close"}
wait 800
cmd {"cmd":"new_space","name":"Work","color":"#3fb950"}
wait 1500
cmd {"cmd":"palette","mode":"new"}
wait 1200
eval palette 'sr010:' + (window.__t = (v) => { const q = document.getElementById('q'); q.value = v; q.dispatchEvent(new Event('input')); return v; }, __t('bh other'))
wait 400
eval palette 'sr10:' + [...document.querySelectorAll('#list .row')].slice(1, 2).map(r => r.querySelector('.t').textContent + '|' + r.querySelector('.s').textContent + '|' + r.querySelector('.hint').textContent).join('')
cmd {"cmd":"palette_close"}
wait 600
eval chrome 'sr011:' + (send({ cmd: 'space', id: state.spaces[0].id }), 'back')
wait 1200
# ---- reload: the arrow turns, never the button ----
cmd {"cmd":"hard_reload"}
wait 120
eval bar 'rl1:' + document.getElementById('reload').className + '|' + getComputedStyle(document.querySelector('#reload svg')).animationName + '|' + getComputedStyle(document.getElementById('reload')).animationName
wait 2500
eval bar 'rl2:' + document.getElementById('reload').className + '|'
cmd {"cmd":"shortcut","key":"r"}
wait 120
eval bar 'rl3:' + document.getElementById('reload').className
wait 2000
# ---- Settings in the page card ----
cmd {"cmd":"open_settings","tab":"general"}
wait 2500
eval settings 'se1:' + document.body.classList.contains('embedded') + '|' + getComputedStyle(document.getElementById('top')).display + '|' + (document.getElementById('sky').width > 0)
eval bar 'se2:' + document.getElementById('url').textContent
shot g01-settings-in-card
eval settings 'se3:' + (document.querySelector('#anim .opt').click(), 'clicked')
wait 900
eval settings 'se4:' + document.getElementById('applied').classList.contains('on') + '|' + document.getElementById('applied').textContent
shot g02-settings-applied
wait 2300
eval settings 'se5:' + document.getElementById('applied').classList.contains('on')
eval settings 'se6:' + (document.querySelector('#anim .opt').click(), 'restored')
wait 600
cmd {"cmd":"shortcut","key":"w"}
wait 1000
eval bar 'se7:' + document.getElementById('url').textContent
eval content 'se8:' + location.pathname
# ---- start page: sections moved ----
cmd {"cmd":"home"}
wait 1500
cmd {"cmd":"set_home_order","order":["recent","favorites"]}
wait 800
eval home 'ho1:' + [...document.getElementById('sections').children].map(c => c.dataset.name).join(',')
eval home 'ho2:' + (() => { document.querySelector('#favorites h2').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', altKey: true, bubbles: true })); return [...document.getElementById('sections').children].map(c => c.dataset.name).join(','); })()
wait 800
eval home 'ho3:' + [...document.getElementById('sections').children].map(c => c.dataset.name).join(',')
eval home 'ho4:' + (() => { const h = document.querySelector('#recent-card h2'), r = h.getBoundingClientRect(), x = r.left + 40, y = r.top + r.height / 2; h.dispatchEvent(new PointerEvent('pointerdown', { pointerId: 7, button: 0, clientX: x, clientY: y, bubbles: true })); for (let i = 1; i <= 8; i++) dispatchEvent(new PointerEvent('pointermove', { pointerId: 7, clientX: x, clientY: y - i * 60, bubbles: true })); const lifted = document.querySelector('#recent-card').classList.contains('lifted') + '|' + document.body.classList.contains('dragging'); dispatchEvent(new PointerEvent('pointerup', { pointerId: 7, clientX: x, clientY: y - 480, bubbles: true })); return lifted + '|' + [...document.getElementById('sections').children].filter(c => !c.hidden).map(c => c.dataset.name).join(','); })()
wait 800
eval home 'ho5:' + [...document.getElementById('sections').children].filter(c => !c.hidden).map(c => c.dataset.name).join(',')
shot g03-home-order
cmd {"cmd":"set_home_order","order":[]}
wait 600
# ---- a cut connection is asked once more, in silence ----
cmd {"cmd":"palette","mode":"url"}
wait 600
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/flaky"}
wait 5000
eval content 'rt1:' + document.title + '|' + location.pathname
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/hop"}
wait 5000
eval content 'rt3:' + document.title + '|' + location.pathname
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/loop"}
wait 5000
eval content 'rt4:' + document.title
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/dead"}
wait 6000
eval content 'rt2:' + document.title
shot g04-dead-after-retry
quit
'@
$script = $script -replace '__PORT__', $port

# Local pages; /flaky cuts the connection once (reset), /dead every time.
$server = Start-Job -ArgumentList $port -ScriptBlock {
    param($port)
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $port)
    $listener.Start()
    $flaky = 0; $hop = 0
    $deadline = (Get-Date).AddMinutes(10)
    while ((Get-Date) -lt $deadline) {
        if (-not $listener.Pending()) { Start-Sleep -Milliseconds 20; continue }
        $client = $listener.AcceptTcpClient()
        try {
            $stream = $client.GetStream()
            $reader = [IO.StreamReader]::new($stream, [Text.Encoding]::ASCII, $false, 4096, $true)
            $path = (($reader.ReadLine()) -split ' ')[1]
            while ($reader.ReadLine()) {}
            $cut = ($path -like '/dead*') -or ($path -like '/flaky*' -and $flaky++ -eq 0) -or ($path -like '/landing*' -and $hop++ -eq 0)
            if ($cut) {
                $client.Client.LingerState = [Net.Sockets.LingerOption]::new($true, 0)
                continue
            }
            $status = '200 OK'; $extra = ''
            if ($path -like '/hop*') { $status = '302 Found'; $extra = "Location: /landing`r`n"; $body = '' }
            elseif ($path -like '/loop*') { $status = '302 Found'; $extra = "Location: /loop`r`n"; $body = '' }
            elseif ($path -like '/favicon*') { $status = '404 Not Found'; $body = 'none' }
            elseif ($path -like '/page*') { $body = '<title>bh page</title><h1>Page</h1>' }
            elseif ($path -like '/login*') { $body = '<title>bh login</title><form><input type=password></form>' }
            elseif ($path -like '/accent*') { $body = '<title>bh R&eacute;glages avanc&eacute;s</title><h1>Accents</h1>' }
            elseif ($path -like '/flaky*') { $body = '<title>bh-retry-ok</title><h1>Second try</h1>' }
            elseif ($path -like '/landing*') { $body = '<title>bh-hop-ok</title><h1>After the redirect</h1>' }
            else { $body = '<title>bh other</title><h1>Other</h1>' }
            $bytes = [Text.Encoding]::UTF8.GetBytes($body)
            $head = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 $status`r`nContent-Type: text/html; charset=utf-8`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n$extra`r`n")
            $stream.Write($head, 0, $head.Length)
            $stream.Write($bytes, 0, $bytes.Length)
        } catch {} finally { $client.Close() }
    }
    $listener.Stop()
}
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii

$logFile = Join-Path $tmp 'bh_run.log'
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(540000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
Stop-Job $server -ErrorAction SilentlyContinue; Remove-Job $server -Force -ErrorAction SilentlyContinue
$log = Get-Content $logFile
$all = @($log | Where-Object { $_ -match '\[dev\] eval ->' })
function Ev($name) { [string]($all | Where-Object { $_ -match "-> `"${name}:" } | Select-Object -First 1) }
$log | Where-Object { $_ -match '\[dev\] (bad|unknown|eval failed|screenshot failed)' } | ForEach-Object { Write-Host $_ }

$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
Check ((Ev sr1) -match 'sr1:2\|0\|1"') 'search: four spellings of two pages give two rows (slash and parameter order folded), the sign-in page never'
Check ((Ev sr2) -match 'sr2:other bh,bh other"') 'search: every word typed must match'
Check ((Ev sr3) -match 'sr3:1"') 'search: nothing but the search row when a word matches nothing'
Check ((Ev sr4) -match 'sr4:bh R.+glages avanc.+s"') 'search: accents do not matter (reglages finds the accented title)'
Check ((Ev sr5) -match 'sr5:bh other"') 'search: one typo is forgiven (otehr finds other)'
Check ((Ev sr6) -match 'sr6:true\|127\.0\.0\.1:47984\|127\.0\.0\.1:47984"') 'search: the field completes the address of an open tab in grey, the first row opens it'
Check ((Ev sr7) -match 'sr7:127\.0\.0\.1:47984"') 'search: Tab takes the completion'
Check ((Ev sr8) -match 'sr8:bh page"') 'search: before any choice, the page visited most comes first'
Check ((Ev sr9) -match 'sr9:bh R.+glages avanc.+s"') 'search: a row chosen for what was typed comes first next time'
Check ((Ev sr10) -match 'sr10:bh other\|127\.0\.0\.1 .+ Personal\|Switch to tab') 'search: a tab of another space is offered with its space'
Check ((Ev rl1) -match 'rl1:spin\|spin\|none"') 'reload: the arrow turns, the button does not'
Check ((Ev rl2) -match 'rl2:\|"') 'reload: the arrow stops once the page has loaded'
Check ((Ev rl3) -match 'rl3:spin"') 'reload: the keyboard reload turns the arrow too'
Check ((Ev se1) -match 'se1:true\|none\|true"') 'settings: in the page card, without a top bar, with the sky of the start page'
Check ((Ev se2) -match 'se2:Settings"') 'settings: the address field says Settings'
Check ((Ev se4) -match 'se4:true\|Changes applied"') 'settings: "Changes applied" after a change'
Check ((Ev se5) -match 'se5:false"') 'settings: the notice leaves after two seconds'
Check ((Ev se7) -notmatch 'Settings' -and (Ev se8) -match 'se8:/other"') 'settings: Mod+W closes them, the tab is still there'
Check ((Ev ho1) -match 'ho1:now,recent,favorites,watches,downloads"') 'start page: the stored order (a missing section keeps its place)'
Check ((Ev ho2) -match 'ho2:now,favorites,recent,') 'start page: Alt+Up moves a section'
Check ((Ev ho3) -match 'ho3:now,favorites,recent,') 'start page: the order moved by keyboard is kept'
Check ((Ev ho4) -match 'ho4:true\|true\|recent,') 'start page: a section dragged by its title lifts and goes up'
Check ((Ev ho5) -match 'ho5:recent,') 'start page: the dragged order is kept'
Check ((Ev rt1) -match 'rt1:bh-retry-ok\|/flaky"') 'unreachable: a connection cut once is asked again in silence'
Check ((Ev rt3) -match 'rt3:bh-hop-ok\|/landing"') 'unreachable: one broken hop after a redirect is asked again in silence'
Check ((Ev rt4) -match 'rt4:Too many redirects"') 'unreachable: a redirect loop is not asked again, it says so'
Check ((Ev rt2) -match 'rt2:Site unreachable"') 'unreachable: a site that keeps failing still gets its page'
# The jump list is written by the shell under the user's Recent folder.
$dests = Join-Path $env:APPDATA 'Microsoft\Windows\Recent\CustomDestinations'
$jump = Get-ChildItem $dests -Filter *.customDestinations-ms -ErrorAction SilentlyContinue |
    Where-Object { $_.LastWriteTime -gt $started } |
    Where-Object { [Text.Encoding]::Unicode.GetString([IO.File]::ReadAllBytes($_.FullName)).Contains('New private window') }
Check ($null -ne $jump) 'jump list: "New window" and "New private window" in the taskbar menu'
Write-Host "screenshots: $(Join-Path $tmp 'bh_shots')"
if ($fail) { exit 1 }
