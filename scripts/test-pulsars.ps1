# Pulsars on Windows (0.8.31), driven by the devtools harness (see crates/app/src/devtools.rs): made
# from a site that announces its feed, lines opened and gone, refreshed, renamed, moved to the
# folders and back, switched off, French; Settings > Pulsars and GitHub (a fake API on this machine).
# Uses the scratch profile %TEMP%\bh_data (wiped first); screenshots land in %TEMP%\bh_shots.
# No network: a local server on 47994. Every eval is named and read by name.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue
$port = 47994
$script = @'
wait 3000
cmd {"cmd":"set_language","language":"en"}
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/"}
wait 2500
# ---- the form, filled with the site shown ----
cmd {"cmd":"action","id":"new_pulsar"}
wait 900
eval palette 'pu1:' + document.body.classList.contains('pulsing') + '|' + document.getElementById('puaddr').value
shot u01-form
cmd {"cmd":"pulsar_new","input":"http://127.0.0.1:__PORT__/none"}
wait 1800
eval palette 'pu2:' + document.body.classList.contains('pulsing') + '|' + document.getElementById('puerr').textContent + '|' + document.getElementById('puaddr').value
eval palette 'pu3:' + (document.getElementById('pulsar').requestSubmit(), 'submit')
wait 2500
eval palette 'pu4:' + document.body.classList.contains('pulsing')
eval chrome 'pu5:' + [...document.querySelectorAll('#tabs > *')].map(n => n.className.split(' ')[0]).join() + '|' + document.querySelector('.ph .fname').textContent + '|' + document.querySelector('.pn').textContent + '|' + [...document.querySelectorAll('.pl .title')].map(n => n.textContent).join(',')
shot u02-sidebar
# ---- a line: opened, and gone ----
eval chrome 'pu6:' + (document.querySelector('.pl').click(), 'open')
wait 2000
eval content 'pu7:' + location.pathname
eval chrome 'pu8:' + document.querySelector('.pn').textContent + '|' + [...document.querySelectorAll('.pl .title')].map(n => n.textContent).join(',')
# ---- its tab stays in the Pulsar: shown, left, gone elsewhere, dropped back on it ----
eval chrome 'st1:' + document.querySelectorAll('.pb .tab.active').length + '|' + document.querySelectorAll('#tabs > .tab').length
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/post99"}
wait 1500
eval chrome 'st2:' + document.querySelector('.pb .tab').classList.contains('active') + '|' + document.querySelector('.pb .tab .title').textContent
eval chrome 'st3:' + (document.querySelector('#tabs > .tab').click(), 'other')
wait 1200
eval chrome 'st4:' + document.querySelectorAll('.pb .tab').length
eval chrome 'st5:' + (() => { const at = (n) => { const r = n.getBoundingClientRect(); return [r.left + 20, r.top + r.height / 2]; }; const ev = (n, t, [x, y]) => n.dispatchEvent(new MouseEvent(t, {button: 0, clientX: x, clientY: y, bubbles: true, cancelable: true})); window.dragTo = (a, b) => { ev(a, 'mousedown', at(a)); ev(document, 'mousemove', [at(a)[0] + 9, at(a)[1]]); ev(document, 'mousemove', at(b)); ev(document, 'mouseup', at(b)); return a.dataset.tab; }; return dragTo(document.querySelector('.pb .tab'), document.querySelector('.ph')); })()
wait 600
eval chrome 'st6:' + document.querySelectorAll('.pb .tab').length
shot u02b-kept
# ---- something new ----
cmd {"cmd":"pulsar_refresh","id":1}
wait 2500
eval chrome 'pu9:' + document.querySelector('.pn').textContent + '|' + document.querySelector('.pl .title').textContent + '|' + document.querySelector('.pl img, .pl b') + '|' + window.hacked
# ---- folded, renamed, with the folders, the menu ----
cmd {"cmd":"pulsar_rename","id":1,"name":"My blog"}
wait 400
eval chrome 'pu10:' + (document.querySelector('.ph').dispatchEvent(new MouseEvent('contextmenu', {clientX: 60, clientY: 200, bubbles: true, cancelable: true})), document.querySelector('.ph .fname').textContent)
wait 900
shot u03-menu
eval palette 'pu11:' + [...document.querySelectorAll('#menu button')].map(b => b.textContent).join(',')
eval palette 'pu12:' + (document.querySelector('#menu button').click(), 'pin')
wait 1200
eval chrome 'pu13:' + document.querySelectorAll('#folders .pulsar').length + '|' + document.querySelectorAll('#tabs .pulsar').length
shot u04-pinned
cmd {"cmd":"pulsar_toggle","id":1}
wait 500
eval chrome 'pu14:' + getComputedStyle(document.querySelector('.pl')).display + '|' + (document.querySelector('.pb .tab').offsetHeight > 0)
cmd {"cmd":"pulsar_toggle","id":1}
cmd {"cmd":"pulsar_pin","id":1,"pinned":false}
wait 500
eval chrome 'pu15:' + document.querySelectorAll('#tabs .pulsar').length
# ---- dragged out among the open tabs: it left ----
eval chrome 'st7:' + dragTo(document.querySelector('.pb .tab'), document.querySelector('#tabs > .tab'))
wait 700
eval chrome 'st8:' + document.querySelectorAll('.pb .tab').length + '|' + document.querySelectorAll('#tabs > .tab').length
eval chrome 'st9:' + (document.querySelector('.pl').click(), 'again')
wait 1500
eval chrome 'st10:' + document.querySelectorAll('.pb .tab').length
# ---- another space has its own ----
cmd {"cmd":"new_space","name":"Second","color":"#ff5c8a"}
wait 1200
eval chrome 'pu16:' + document.querySelectorAll('.pulsar').length
cmd {"cmd":"space","id":1}
wait 900
# ---- Settings: off, on ----
cmd {"cmd":"open_settings","tab":"pulsars"}
wait 1500
eval settings 'pu17:' + document.getElementById('pullist').textContent + '|' + (document.querySelectorAll('#pulopts .opt').length, document.querySelector('#pulopts .opt').getAttribute('aria-checked'))
wait 500
shot u05-settings
eval settings 'pu18:' + (document.querySelector('#pulopts .opt').click(), 'off')
wait 800
eval chrome 'pu19:' + document.querySelectorAll('.pulsar').length
cmd {"cmd":"pulsar_refresh","id":1}
wait 6500
eval settings 'pu20:' + (document.querySelector('#pulopts .opt').click(), 'on')
cmd {"cmd":"set_language","language":"fr"}
wait 800
cmd {"cmd":"close_settings"}
wait 600
eval chrome 'pu21:' + document.querySelectorAll('.pulsar').length + '|' + [...document.querySelectorAll('.pl .when')].every(n => n.offsetWidth > 0)
shot u06-fr
cmd {"cmd":"pulsar_read_all","id":1}
wait 500
eval chrome 'pu22:' + document.querySelector('.pulsar').classList.contains('lit') + '|' + document.querySelectorAll('.pl').length
cmd {"cmd":"pulsar_remove","id":1}
wait 500
eval chrome 'pu23:' + document.querySelectorAll('.pulsar').length + '|' + document.querySelectorAll('#tabs > .tab').length
cmd {"cmd":"set_language","language":"en"}
# ---- GitHub: a wrong token, the right one, a search ----
cmd {"cmd":"open_settings","tab":"pulsars"}
wait 1200
eval settings 'gh1:' + document.getElementById('ghon').hidden + '|' + (document.getElementById('ghtoken').value = 'ghp_wrongwrongwrongwrong', document.getElementById('ghsave').click(), document.getElementById('ghtoken').value.length)
wait 1500
eval settings 'gh2:' + document.getElementById('ghstatus').textContent + '|' + document.getElementById('ghon').hidden
eval settings 'gh3:' + (document.getElementById('ghtoken').value = '__TOKEN__', document.getElementById('ghsave').click(), 'connect')
wait 2500
eval settings 'gh4:' + document.getElementById('ghaccount').textContent + '|' + document.getElementById('ghoff').hidden + '|' + document.documentElement.outerHTML.includes('__TOKEN__')
eval settings 'gh5:' + (document.getElementById('pulrepo').value = 'not a repo', document.getElementById('ghadd').click(), 'bad')
wait 700
eval settings 'gh6:' + document.getElementById('ghstatus').textContent
eval settings 'gh7:' + (document.getElementById('ghadd').click(), 'add')
wait 2500
shot u07-settings-github
eval chrome 'gh8:' + document.querySelector('.ph .fname').textContent + '|' + document.querySelector('.pn').textContent + '|' + [...document.querySelectorAll('.pl')].map(n => n.textContent).join(',')
eval settings 'gh9:' + (() => { const m = document.querySelector('main'); return m.scrollWidth <= m.clientWidth + 1; })() + '|' + document.querySelectorAll('#pullist select').length + '|' + document.querySelector('#pullist .sub').textContent
cmd {"cmd":"close_settings"}
eval chrome 'gh10:' + (document.querySelector('.pl').click(), 'open')
wait 2000
eval chrome 'gh11:' + document.querySelector('.pn').textContent + '|' + document.querySelectorAll('.pl').length + '|' + document.querySelectorAll('.pb .tab.active').length
shot u08-github
# ---- merged there: gone here ----
cmd {"cmd":"pulsar_refresh","id":1}
wait 2500
eval chrome 'gh12:' + [...document.querySelectorAll('.pl .title')].map(n => n.textContent).join(',')
eval settings 'gh16:' + (() => { const s = document.querySelector('#pullist select'); window.was = s.value; s.value = [...s.options].find(o => o.value !== was).value; s.dispatchEvent(new Event('change', {bubbles: true})); return 'moved'; })()
wait 800
eval chrome 'gh13:' + document.querySelectorAll('.pulsar').length
eval settings 'gh17:' + (() => { const s = document.querySelector('#pullist select'); s.value = was; s.dispatchEvent(new Event('change', {bubbles: true})); return 'back'; })()
wait 800
eval chrome 'gh18:' + document.querySelectorAll('.pulsar').length
# ---- an organization with a sign-on: said, with a button; gone with its Pulsar ----
cmd {"cmd":"pulsar_github_new","kind":"mine","repo":"corp/secret"}
wait 2500
eval settings 'so1:' + document.getElementById('ghstatus').textContent + '|' + document.getElementById('ghsso').hidden + '|' + [...document.querySelectorAll('#pullist .sub')].pop().textContent
shot u08b-sso
cmd {"cmd":"pulsar_remove","id":2}
wait 700
eval settings 'so2:' + document.getElementById('ghstatus').textContent + '|' + document.getElementById('ghsso').hidden
cmd {"cmd":"set_pulsar_lines","lines":20}
cmd {"cmd":"set_pulsar_every","minutes":60}
cmd {"cmd":"set_pulsar_every","minutes":1}
cmd {"cmd":"open_settings","tab":"pulsars"}
wait 1200
eval settings 'gh14:' + [...document.querySelectorAll('#pulopts .seg .on')].map(n => n.textContent).join()
cmd {"cmd":"set_language","language":"fr"}
wait 700
shot u09-settings-fr
cmd {"cmd":"set_language","language":"en"}
cmd {"cmd":"pulsar_github_disconnect"}
wait 700
eval settings 'gh15:' + document.getElementById('ghon').hidden + '|' + document.querySelector('#pullist .sub').textContent
eval chrome 'alive:1'
quit
'@
# Not a secret: the fake API only checks it receives this.
$token = 'ghp_' + [Guid]::NewGuid().ToString('N')
$script = $script -replace '__PORT__', $port -replace '__TOKEN__', $token
$server = Start-Job -ArgumentList $port, $token -ScriptBlock {
    param($port, $token)
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $port)
    $listener.Start()
    $feeds = 0; $searches = 0
    $deadline = (Get-Date).AddMinutes(9)
    while ((Get-Date) -lt $deadline) {
        if (-not $listener.Pending()) { Start-Sleep -Milliseconds 20; continue }
        $client = $listener.AcceptTcpClient()
        $client.ReceiveTimeout = 2000
        try {
            $stream = $client.GetStream()
            $reader = [IO.StreamReader]::new($stream, [Text.Encoding]::ASCII, $false, 4096, $true)
            $path = ($reader.ReadLine() -split ' ')[1]
            $cookie = $false; $auth = ''
            while ($line = $reader.ReadLine()) { if ($line -match '^Cookie:') { $cookie = $true }; if ($line -match '^Authentication: (.*)$|^Authorization: (.*)$') { $auth = $Matches[2] } }
            $status = '200 OK'; $type = 'text/html; charset=utf-8'; $extra = ''
            if ($path -eq '/') {
                $extra = "Set-Cookie: bh=1; Path=/`r`n"
                $body = '<title>bh site</title><link rel="alternate" type="application/rss+xml" href="/rss"><h1>Site</h1>'
            }
            elseif ($path -eq '/rss') {
                $feeds++
                "rss cookie=$cookie"
                $type = 'application/rss+xml'
                $now = (Get-Date).ToUniversalTime()
                $items = 1..9 | ForEach-Object { "<item><title>Post $_</title><link>/post$_</link><guid>g$_</guid><pubDate>$($now.AddHours(-30 + $_ * 3).ToString('r'))</pubDate></item>" }
                if ($feeds -gt 1) { $items += "<item><title>&lt;img src=x onerror=&quot;hacked=1&quot;&gt;Fresh &amp;amp; new</title><link>/fresh</link><guid>f</guid><pubDate>$($now.ToString('r'))</pubDate></item>" }
                $body = "<rss version=`"2.0`"><channel><title>The blog</title>$($items -join '')</channel></rss>"
            }
            elseif ($path -like '/api/*') {
                $type = 'application/json'
                "api $path $($auth -eq "Bearer $token")"
                if ($auth -ne "Bearer $token") { $status = '401 Unauthorized'; $body = '{}' }
                elseif ($path -eq '/api/user') { $body = '{"login":"octo"}' }
                elseif ($path -match 'corp') {
                    $status = '422 Unprocessable Entity'; $body = '{}'
                    if ($path -like '/api/repos/*') { $status = '403 Forbidden'; $extra = "X-GitHub-SSO: required; url=https://github.com/orgs/corp/sso?authorization_request=A`r`n" }
                }
                else {
                    $searches++
                    $list = @('{"html_url":"http://127.0.0.1:PORT/post2","title":"Second PR","number":7,"updated_at":"2026-06-30T08:00:00Z","repository_url":"https://api.github.com/repos/o/wry"}')
                    if ($searches -eq 1) { $list = @('{"html_url":"http://127.0.0.1:PORT/post1","title":"First PR","number":12,"updated_at":"2026-07-01T08:00:00Z","repository_url":"https://api.github.com/repos/o/wry"}') + $list }
                    $body = ('{"items":[' + ($list -join ',') + ']}').Replace('PORT', $port)
                }
            }
            elseif ($path -match '^/(post|fresh)') { $body = "<title>bh $path</title><h1>$path</h1>" }
            elseif ($path -eq '/none') { $body = '<title>none</title><p>no feed</p>' }
            else { $status = '404 Not Found'; $body = 'no' }
            $bytes = [Text.Encoding]::UTF8.GetBytes($body)
            $head = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 $status`r`nContent-Type: $type`r`n${extra}Content-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n")
            $stream.Write($head, 0, $head.Length)
            $stream.Write($bytes, 0, $bytes.Length)
        } catch {} finally { $client.Close() }
    }
}
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii
$logFile = Join-Path $tmp 'bh_run.log'
$env:BH_GITHUB_API = "http://127.0.0.1:$port/api"
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(500000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
Remove-Item Env:BH_GITHUB_API
$served = @(Receive-Job $server -ErrorAction SilentlyContinue)
$apiCalls = @($served | Where-Object { $_ -like 'api *' })
$served = @($served | Where-Object { $_ -like 'rss *' })
Stop-Job $server; Remove-Job $server -Force
$log = Get-Content $logFile -Encoding utf8
$all = @($log | Where-Object { $_ -match '\[dev\] eval ->' })
function Ev($name) { [string]($all | Where-Object { $_ -match "-> `"${name}:" } | Select-Object -First 1) }
$log | Where-Object { $_ -match '\[dev\] (bad|unknown|eval failed|screenshot failed)|panicked' } | ForEach-Object { Write-Host $_ }
$all | ForEach-Object { Write-Host "  $_" }
$served | ForEach-Object { Write-Host "  $_" }
$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
Check ((Ev pu1) -match "pu1:true\|http://127\.0\.0\.1:$port/") 'the form opens with the address of the site shown'
Check ((Ev pu2) -match "pu2:true\|This site publishes no feed.*\|http://127\.0\.0\.1:$port/") 'a site without a feed says so, the address typed stays'
Check ((Ev pu4) -match 'pu4:false') 'made: the form closes'
Check ((Ev pu5) -match 'pu5:pulsar,tab\|The blog\|5\|Post 9,Post 8,Post 7,Post 6,Post 5') 'first among the open tabs, with the five newest entries'
Check ((Ev pu7) -match 'pu7:/post9') 'a line opens its page'
Check ((Ev pu8) -match 'pu8:4\|Post 8,Post 7,Post 6,Post 5') 'and leaves'
Check ((Ev pu9) -match 'pu9:5\|Fresh & new\|null\|undefined') 'a new entry comes first, its markup never run'
Check ((Ev pu10) -match 'pu10:My blog') 'renamed'
Check ((Ev pu11) -match 'pu11:Move to the folders,Refresh now,Mark everything as read,Rename the Pulsar,New Pulsar,Pulsar settings,Delete the Pulsar') 'its menu'
Check ((Ev pu13) -match 'pu13:1\|0') 'moved to the folders from its menu'
Check ((Ev pu14) -match 'pu14:none\|true') 'folded, the page open under it stays'
Check ((Ev pu15) -match 'pu15:1') 'back with the open tabs'
Check ((Ev pu16) -match 'pu16:0') 'each space has its own'
Check ((Ev pu17) -match 'pu17:MMy blog.*4.*\|true') 'Settings list it'
Check ((Ev pu19) -match 'pu19:0') 'switched off, it leaves the sidebar'
Check ($served.Count -eq 2) 'and its feed is not asked any more'
Check (@($served | Where-Object { $_ -match 'True' }).Count -eq 0) 'the feed is asked without the cookies of the site'
Check ((Ev pu21) -match 'pu21:1\|true') 'switched on, it is back'
Check ((Ev pu22) -match 'pu22:false\|0') 'everything read: only its open tab is left'
Check ((Ev pu23) -match 'pu23:0\|3') 'deleted, its tab joins the open ones'
Check ((Ev st1) -match 'st1:1\|1') 'the tab opened from a line is in the Pulsar'
Check ((Ev st2) -match 'st2:true\|bh /post99') 'it stays there when it goes elsewhere'
Check ((Ev st4) -match 'st4:1') 'and when another tab is shown'
Check ((Ev st6) -match 'st6:1') 'dropped on its own Pulsar, it stays'
Check ((Ev st8) -match 'st8:0\|2') 'dragged out, it is an open tab'
Check ((Ev st10) -match 'st10:1') 'the next line has its tab there'
$apiCalls | ForEach-Object { Write-Host "  $_" }
Check ((Ev gh1) -match 'gh1:true\|0') 'no account: the token field, emptied once sent'
Check ((Ev gh2) -match 'gh2:GitHub refused the token\|true') 'a wrong token is refused'
Check ((Ev gh4) -match 'gh4:Connected as octo.*\|true\|false') 'connected, the token not in the page'
Check ((Ev gh6) -match 'gh6:Type the repository as owner/name') 'a bad repository is refused'
Check ((Ev gh8) -match 'gh8:To review\|2\|First PRwry #12,Second PRwry #7') 'the Pulsar lists what GitHub found'
Check ((Ev gh9) -match 'gh9:true\|1\|GitHub . To review') 'Settings list it, with a space to choose'
Check ((Ev gh11) -match 'gh11:1\|1\|1') 'opened, a line becomes its tab'
Check ((Ev gh12) -match 'gh12:Second PR"') 'dealt with on GitHub, it leaves'
Check ((Ev gh13) -match 'gh13:0') 'moved to another space'
Check ((Ev gh18) -match 'gh18:1') 'and back'
Check ((Ev so1) -match 'so1:The organization corp asks for its sign-on.*\|false\|The organization corp') 'an organization with a sign-on says so, with its button'
Check ((Ev so2) -match 'so2:\|true') 'removed, nothing is left of it'
Check ((Ev gh14) -match 'gh14:1 h,20') 'the options are kept, a wrong delay refused'
Check ((Ev gh15) -match 'gh15:true\|Connect GitHub in Settings') 'disconnected, the Pulsar says so'
Check (@($apiCalls | Where-Object { $_ -match 'search/issues\?.*review-requested' }).Count -eq 2) 'one search per reading'
Check ((Ev alive) -match 'alive:1') 'the app is alive at the end'
if ($fail) { exit 1 }
