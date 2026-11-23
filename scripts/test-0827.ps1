# 0.8.27 scenarios on Windows, driven by the devtools harness (see crates/app/src/devtools.rs):
# the shield counter on Windows (trackers of the page, of a frame, of a script), a notice taken
# away by a click, the "Show in folder" button of a finished download, the Library without its
# Spaces section, the tabs to tidy lit in the sidebar, the private-window extension option.
# Uses the scratch profile %TEMP%\bh_data (wiped first); screenshots land in %TEMP%\bh_shots.
# No network needed: the trackers are refused by the shell before any request leaves (403).
# A local server on 47987. Every eval is named ("tk1:", "nt1:"…) and read by name.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue
$port = 47987

$script = @'
wait 3000
cmd {"cmd":"set_language","language":"en"}
# ---- the shield counter: trackers of the page, of a frame and of a script ----
eval chrome 'tk0:' + state.blocked
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/trk"}
wait 4000
eval content 'tk1:' + document.title + '|' + document.querySelectorAll('img').length
eval chrome 'tk2:' + state.blocked + '|' + document.getElementById('blockedn').textContent.trim()
shot k01-shield-counter
cmd {"cmd":"reload"}
wait 3000
eval chrome 'tk3:' + state.blocked
# A tracker inside a frame of its own process only (ads and widgets of other sites live there).
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/onlyframe"}
wait 3000
eval chrome 'tk4:' + state.blocked
eval content 'tkf:' + document.title
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/sameframe"}
wait 3000
eval chrome 'tk5:' + state.blocked
# ---- a notice without buttons goes away when clicked ----
eval chrome 'nt1:' + (bh.notice({ text: 'Plain notice', icon: 'info', tone: 'info', stay: 6000, actions: [] }), document.getElementById('toast').classList.contains('show'))
wait 300
eval chrome 'nt2:' + (document.getElementById('toast').click(), document.getElementById('toast').classList.contains('show'))
eval chrome 'nt3:' + (bh.notice({ text: 'Asks', icon: 'undo', tone: 'info', stay: 6000, actions: [{ label: 'Undo', run: { cmd: 'noop' } }] }), document.getElementById('toast').click(), document.getElementById('toast').classList.contains('show'))
eval chrome 'nt4:' + (bh.noticeDone(), 'done')
# ---- tidy up: the card lights the tabs it would file; an unticked site stays loose ----
eval chrome 'td1:' + (() => { const id = state.tabs.find(t => !t.pinned && t.folder == null).id; renderTidy({ tabs: 3, groups: [{ name: 'example.test', count: 3, tabs: [id] }, { name: 'other.test', count: 3, tabs: [] }] }); const card = document.querySelector('#tidy .td'); card.dispatchEvent(new MouseEvent('mouseenter')); const lit = document.querySelectorAll('.tab.tidy-lit').length; document.querySelector('#tidy .g').click(); return lit + '|' + document.querySelectorAll('.tab.tidy-lit').length + '|' + document.querySelector('#tidy .q').textContent; })()
eval chrome 'td2:' + (() => { document.querySelectorAll('#tidy .g')[1].click(); const go = document.querySelector('#tidy button.go'); const r = go.disabled; renderTidy(null); return r + '|' + document.querySelectorAll('.tab.tidy-lit').length; })()
# ---- a downloaded text file opens in a tab (the engine's own viewer) ----
cmd {"cmd":"open","input":"__VIEWFILE__"}
wait 2000
eval content 'vw1:' + location.protocol + '|' + document.body.innerText.trim()
# ---- downloads: the finished card has a "Show in folder" button ----
eval chrome 'dl1:' + (bh.downloads([{ id: 991, name: 'bh-file.bin', received: 10, total: 10, state: 'ok' }]), (() => { const f = document.querySelector('.dl .cl.fo'); return !!f && getComputedStyle(f).display + '|' + f.title; })())
eval chrome 'dl2:' + (bh.downloads([{ id: 992, name: 'bh-bad.bin', received: 1, total: 10, state: 'bad' }]), getComputedStyle(document.querySelector('.dl.bad .cl.fo')).display)
eval chrome 'dl3:' + (bh.downloads([]), 'cleared')
# ---- the Library has no Spaces section any more ----
cmd {"cmd":"open_library"}
wait 2000
eval library 'lb1:' + !document.querySelector('.nav[data-s="spaces"]') + '|' + document.querySelectorAll('.nav').length
cmd {"cmd":"close_library"}
wait 600
# ---- private windows: the extension settings option shows under "Run in private windows" ----
cmd {"cmd":"set_extensions_in_private","on":true}
cmd {"cmd":"open_settings","tab":"extensions"}
wait 2500
eval settings 'pv1:' + document.getElementById('extpriv').querySelectorAll('.row, .opt, [role="switch"]').length + '|' + document.getElementById('extpriv').textContent.includes('settings of normal windows')
cmd {"cmd":"set_extensions_in_private","on":false}
wait 500
eval settings 'pv2:' + document.getElementById('extpriv').textContent.includes('settings of normal windows')
cmd {"cmd":"close_settings"}
wait 500
eval chrome 'alive'
quit
'@
$viewFile = Join-Path $tmp 'bh-view-0827.txt'
Set-Content -Path $viewFile -Value 'bh viewer 0827' -Encoding ascii
$script = $script -replace '__PORT__', $port
$script = $script -replace '__VIEWFILE__', ([Uri]$viewFile).AbsoluteUri

$server = Start-Job -ArgumentList $port -ScriptBlock {
    param($port)
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $port)
    $listener.Start()
    $deadline = (Get-Date).AddMinutes(10)
    while ((Get-Date) -lt $deadline) {
        if (-not $listener.Pending()) { Start-Sleep -Milliseconds 20; continue }
        $client = $listener.AcceptTcpClient()
        try {
            $stream = $client.GetStream()
            $reader = [IO.StreamReader]::new($stream, [Text.Encoding]::ASCII, $false, 4096, $true)
            $path = (($reader.ReadLine()) -split ' ')[1]
            while ($reader.ReadLine()) {}
            $status = '200 OK'
            if ($path -like '/favicon*') { $status = '404 Not Found'; $body = 'none' }
            elseif ($path -like '/trk*') {
                # Three trackers in the page, one in a sandboxed frame (its own process), one added by a script.
                $body = '<title>bh trackers</title><h1>Trackers</h1>' +
                    '<img src="https://www.google-analytics.com/collect?v=1">' +
                    '<img src="https://stats.g.doubleclick.net/pixel.gif">' +
                    '<script src="https://connect.facebook.net/en_US/fbevents.js"></script>' +
                    '<iframe sandbox="allow-scripts" src="/frame"></iframe>' +
                    '<script>setTimeout(()=>{const i=new Image();i.src="https://sb.scorecardresearch.com/p?c1=2";document.body.append(i)},300)</script>'
            }
            elseif ($path -like '/sameframe*') { $body = '<title>bh same</title><iframe src="/frame"></iframe>' }
            elseif ($path -like '/onlyframe*') { $body = '<title>bh frame</title><script>addEventListener("message",e=>{document.title+="|"+e.data})</script><iframe sandbox="allow-scripts" src="/frame"></iframe>' }
            elseif ($path -like '/frame*') { $body = '<img src="https://www.googletagmanager.com/gtm.js?id=x" onerror="parent.postMessage(''img-error'',''*'')" onload="parent.postMessage(''img-load'',''*'')"><script>parent.postMessage("frame-ok","*")</script>' }
            else { $body = '<title>bh other</title><h1>Other</h1>' }
            $bytes = [Text.Encoding]::UTF8.GetBytes($body)
            $head = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 $status`r`nContent-Type: text/html; charset=utf-8`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n")
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
$all | ForEach-Object { Write-Host "  $_" }

Remove-Item $viewFile -ErrorAction SilentlyContinue
$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
$before = if ((Ev tk0) -match 'tk0:(\d+)') { [int]$Matches[1] } else { -1 }
$after = if ((Ev tk2) -match 'tk2:(\d+)') { [int]$Matches[1] } else { -1 }
$again = if ((Ev tk3) -match 'tk3:(\d+)') { [int]$Matches[1] } else { -1 }
Check ((Ev tk1) -match 'tk1:bh trackers\|') 'the page with trackers is shown'
Check ($after -ge $before + 5) "the shield counter counts the trackers of the page, its frame and its script ($before -> $after)"
Check ($again -gt $after) "a reload counts them again ($after -> $again)"
$framed = if ((Ev tk4) -match 'tk4:(\d+)') { [int]$Matches[1] } else { -1 }
Check ($framed -gt $again) "a tracker inside a frame of its own process (sandboxed, like ads of other sites) is refused and counted ($again -> $framed)"
Check ((Ev tkf) -match 'frame-ok\|img-error') 'the frame runs, its tracker is refused'
$same = if ((Ev tk5) -match 'tk5:(\d+)') { [int]$Matches[1] } else { -1 }
Check ($same -gt $framed) "a tracker in a frame of the same site is counted ($framed -> $same)"
Check ((Ev nt1) -match 'nt1:true') 'a notice shows'
Check ((Ev nt2) -match 'nt2:false') 'a notice without buttons goes away when clicked'
Check ((Ev nt3) -match 'nt3:true') 'a notice with a button stays when clicked beside it'
Check ((Ev td1) -match 'td1:1\|0\|Tidy up 3 tabs into 1 folder\?') 'tidy: the card lights its tabs; an unticked site goes dark and the question counts again'
Check ((Ev td2) -match 'td2:true\|0') 'tidy: nothing ticked, nothing to do; the light goes with the card'
Check ((Ev vw1) -match 'vw1:file:\|bh viewer 0827') 'a text file of the disk opens in a tab'
Check ((Ev dl1) -match 'dl1:grid\|Show in folder') 'a finished download card: Show in folder'
Check ((Ev dl2) -match 'dl2:none') 'a failed download card: no folder button'
Check ((Ev lb1) -match 'lb1:true\|7') 'the Library: no Spaces section, seven sections'
Check ((Ev pv1) -match 'pv1:\d+\|true') 'Settings: the extension settings option under "Run in private windows"'
Check ((Ev pv2) -match 'pv2:false') 'the option hides when extensions are off in private windows'
Check ((Ev alive) -ne '' -or ($all -join '') -match 'alive') 'no crash: the app is still running'
if ($fail) { exit 1 }
