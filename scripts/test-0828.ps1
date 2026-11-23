# 0.8.28 scenarios on Windows, driven by the devtools harness (see crates/app/src/devtools.rs):
# the Shields report of the Library (grade, tiles, refused hosts with their owner, companies,
# allow and block again, live count while the Library is open, the filter, a narrow window).
# Uses the scratch profile %TEMP%\bh_data (wiped first); screenshots land in %TEMP%\bh_shots.
# No network needed: the trackers are refused by the shell before any request leaves (403).
# A local server on 47988. Every eval is named ("sh1:"…) and read by name.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue
$port = 47988

$script = @'
wait 3000
cmd {"cmd":"set_language","language":"en"}
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/trk"}
wait 4000
eval chrome 'sh0:' + state.blocked
# ---- the report of the site shown, as the shield's menu opens it ----
cmd {"cmd":"open_library","section":"shields"}
wait 2500
eval library 'sh1:' + document.querySelector('.nav.on').dataset.s + '|' + document.querySelector('.shhead h1').textContent + '|' + document.querySelector('.ring b').textContent + '|' + document.querySelectorAll('.shrow').length + '|' + document.querySelector('.shsite.on .nm').textContent
eval library 'sh2:' + [...document.querySelectorAll('.shrow')].map(r => r.querySelector('.h').textContent + '=' + r.querySelector('.k').textContent).join(',')
eval library 'sh3:' + [...document.querySelectorAll('.bar')].map(b => b.firstChild.textContent + '=' + b.lastChild.textContent).join(',')
eval library 'sh4:' + document.querySelectorAll('.shtile').length + '|' + [...document.querySelectorAll('.shtile span')].map(s => s.textContent).join(',') + '|' + document.querySelectorAll('.shbox li').length
eval library 'sh5:' + document.querySelector('.count[data-c="shields"]').textContent
shot s01-shields-report
# ---- allow a host from the report, then block it again ----
eval library 'al1:' + (() => { const r = [...document.querySelectorAll('.shrow')].find(r => r.querySelector('.h').textContent.startsWith('www.google-analytics.com')); r.querySelector('button').click(); return 'clicked'; })()
wait 800
eval library 'al2:' + (() => { const r = [...document.querySelectorAll('.shrow')].find(r => r.querySelector('.h').textContent.startsWith('www.google-analytics.com')); return !!r.querySelector('.chip.allow') + '|' + r.querySelector('button').textContent; })()
eval library 'al3:' + (() => { const r = [...document.querySelectorAll('.shrow')].find(r => r.querySelector('.h').textContent.startsWith('www.google-analytics.com')); r.querySelector('button').click(); return 'clicked'; })()
wait 800
eval library 'al4:' + (() => { const r = [...document.querySelectorAll('.shrow')].find(r => r.querySelector('.h').textContent.startsWith('www.google-analytics.com')); return !!r.querySelector('.chip.allow') + '|' + r.querySelector('button').textContent; })()
# ---- the filter: ads only, then all again ----
eval library 'fl1:' + (() => { const all = document.querySelectorAll('.shrow').length; [...document.querySelectorAll('.shlist .seg button')][2].click(); const ads = document.querySelectorAll('.shrow').length; [...document.querySelectorAll('.shlist .seg button')][0].click(); return all + '|' + ads + '|' + document.querySelectorAll('.shrow').length; })()
# ---- the page blocks more while the Library is open (12 s after it loaded): the report follows ----
eval library 'lv0:' + document.querySelector('.count[data-c="shields"]').textContent
wait 5000
eval chrome 'lv2:' + state.blocked
eval library 'lv1:' + document.querySelector('.count[data-c="shields"]').textContent + '|' + document.querySelector('.shsite.on .n').textContent
# ---- a narrow window: one column, no page scrolling sideways ----
eval library 'nw1:' + (() => { const m = document.getElementById('m'); return m.scrollWidth <= m.clientWidth + 1; })()
cmd {"cmd":"close_library"}
wait 600
eval chrome 'alive'
quit
'@
$script = $script -replace '__PORT__', $port

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
                $body = '<title>bh trackers</title><h1>Trackers</h1>' +
                    '<img src="https://www.google-analytics.com/collect?v=1">' +
                    '<img src="https://stats.g.doubleclick.net/pixel.gif">' +
                    '<script src="https://connect.facebook.net/en_US/fbevents.js"></script>' +
                    '<script>setTimeout(()=>{const i=new Image();i.src="https://sb.scorecardresearch.com/p?c1=2";document.body.append(i)},300)</script>' +
                    # Two more, later: they arrive while the Library is open.
                    '<script>setTimeout(()=>{for(const u of ["https://bat.bing.com/action/0","https://www.googletagmanager.com/gtm.js?id=late"]){const i=new Image();i.src=u;document.body.append(i)}},12000)</script>'
            }
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

$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
$blocked = if ((Ev sh0) -match 'sh0:(\d+)') { [int]$Matches[1] } else { -1 }
Check ($blocked -ge 4) "the page's trackers are refused ($blocked)"
Check ((Ev sh1) -match 'sh1:shields\|127\.0\.0\.1\|[B-E]\|[4-9]\|127\.0\.0\.1') 'the Shields section opens on the site shown: grade, hosts, the site picked on the left'
Check ((Ev sh2) -match 'www\.google-analytics\.com=Google .+ Analytics' -and (Ev sh2) -match 'connect\.facebook\.net=Meta .+ Social network' -and (Ev sh2) -match 'doubleclick\.net=Google .+ Advertising') 'each refused host says who owns it and what it does'
Check ((Ev sh3) -match 'sh3:Google=2,') 'the companies: Google first with its two hosts'
Check ((Ev sh4) -match 'sh4:5\|Trackers,Ads,Links cleaned \(session\),Cookie banners,Fingerprint\|[3-9]') 'five tiles and the reasons of the grade'
Check ((Ev sh5) -match "sh5:$blocked") 'the rail counts what the session refused'
Check ((Ev al2) -match 'al2:true\|Block again') 'Allow: the host is marked allowed and can be blocked again'
Check ((Ev al4) -match 'al4:false\|Allow') 'Block again: the host is refused again'
Check ((Ev fl1) -match 'fl1:(\d+)\|(\d+)\|\1' -and [int]($Matches[2]) -ge 1 -and [int]($Matches[2]) -lt [int]($Matches[1])) 'the Ads filter keeps the ad hosts only, All brings them back'
$live0 = if ((Ev lv0) -match 'lv0:(\d+)') { [int]$Matches[1] } else { -1 }
$live1 = if ((Ev lv1) -match 'lv1:(\d+)') { [int]$Matches[1] } else { -1 }
$shell = if ((Ev lv2) -match 'lv2:(\d+)') { [int]$Matches[1] } else { -2 }
Check ($live1 -gt $live0 -and $live1 -eq $shell) "the open report follows new refusals ($live0 -> $live1, shell $shell)"
Check ((Ev nw1) -match 'nw1:true') 'no sideways scrolling'
Check ((Ev alive) -ne '' -or ($all -join '') -match 'alive') 'no crash: the app is still running'
if ($fail) { exit 1 }
