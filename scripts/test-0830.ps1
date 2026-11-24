# 0.8.30 scenarios on Windows, driven by the devtools harness (see crates/app/src/devtools.rs):
# email aliases (Settings section, a fake SimpleLogin on this machine, the menu of an email field,
# the alias written and kept, the mark in the password list, switched off, deleted).
# Uses the scratch profile %TEMP%\bh_data (wiped first); screenshots land in %TEMP%\bh_shots.
# Network: the last step only (the page of the key). A local server on 47990, which plays the service and the sites. The menu only
# answers real input (isTrusted): a job types it through the engine's DevTools port (47991).
# Every eval is named ("al1:"…) and read by name.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue
$port = 47990
$cdp = 47991
$apiLog = Join-Path $tmp 'bh_alias_api.log'
Set-Content -Path $apiLog -Value '' -Encoding ascii
# Not a secret: the fake service only checks it receives this.
$key = 'bhtest' + [Guid]::NewGuid().ToString('N')

$script = @'
wait 3000
cmd {"cmd":"set_language","language":"en"}
# ---- nothing connected: no menu, nothing sent ----
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/signup-before"}
wait 3000
eval content 'al0:' + (document.getElementById('e').focus(), document.title)
wait 800
# ---- Settings > Email aliases ----
cmd {"cmd":"open_settings","tab":"aliases"}
wait 2000
eval settings 'al1:' + document.querySelector('section.on').id + '|' + document.getElementById('alopen').hidden + '|' + [...document.querySelectorAll('#alprov .label')].map(x => x.textContent).join(',') + '|' + document.getElementById('aloptscard').hidden
shot a01-aliases-empty
cmd {"cmd":"aliases","action":{"op":"connect","provider":"simplelogin","server":"http://127.0.0.1:__PORT__","key":"wrong"}}
wait 1500
eval settings 'al2:' + document.getElementById('alstatus').textContent + '|' + document.getElementById('alon').hidden
cmd {"cmd":"aliases","action":{"op":"connect","provider":"simplelogin","server":"http://mail.example.com","key":"wrong"}}
wait 600
eval settings 'al3:' + document.getElementById('alstatus').textContent
eval settings 'al4:' + (document.getElementById('alserver').value = 'http://127.0.0.1:__PORT__', document.getElementById('alkey').value = '__KEY__', document.getElementById('alsave').click(), 'connect')
wait 1500
eval settings 'al5:' + document.getElementById('alstatus').textContent + '|' + document.getElementById('alon').hidden + '|' + document.getElementById('alkey').value.length + '|' + document.documentElement.outerHTML.includes('__KEY__')
shot a02-aliases-connected
cmd {"cmd":"close_settings"}
wait 600
# ---- a page posting the command itself gets nothing ----
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/signup"}
wait 2500
eval content 'al6:' + (window.__bhipc.postMessage(JSON.stringify({cmd: 'autofill_query', login: false, email: true})), window.__bhipc.postMessage(JSON.stringify({cmd: 'autofill_alias', token: 'guess'})), window.__bhipc.postMessage(JSON.stringify({cmd: 'autofill_alias'})), 'forged')
wait 1500
eval content 'al7:' + document.getElementById('e').value
# ---- the menu, with real input: the job clicks the field, then Down and Enter ----
eval content 'al8:' + (document.title = 'bh go', 'go')
wait 2500
shot a03-alias-menu
wait 5000
eval content 'al9:' + document.getElementById('e').value
eval chrome 'al10:' + document.getElementById('toast').textContent
shot a04-alias-filled
# ---- the login saved with it is marked; another login is not ----
cmd {"cmd":"passwords","action":{"op":"save","origin":"shop.example","username":"first.k7@alias.example","password":"pw-1"}}
cmd {"cmd":"passwords","action":{"op":"save","origin":"bank.example","username":"me@mail.example","password":"pw-2"}}
cmd {"cmd":"open_settings","tab":"passwords"}
wait 1500
eval settings 'mp1:' + getComputedStyle(document.getElementById('pwmasteron')).display + '|' + getComputedStyle(document.getElementById('pwsetmaster')).display
eval settings 'al11:' + [...document.querySelectorAll('#pwlist .row')].map(r => r.querySelector('.name').textContent + '=' + !!r.querySelector('.chip.alias')).join(',')
shot a05-passwords-chip
# ---- the list: switched off, then deleted ----
cmd {"cmd":"open_settings","tab":"aliases"}
wait 1200
eval settings 'al12:' + document.querySelectorAll('#allist .row').length + '|' + document.getElementById('allist').textContent
shot a06-aliases-list
eval settings 'al13:' + (document.querySelector('#allist .opt').click(), 'off')
wait 1500
eval settings 'al14:' + document.querySelector('#allist .row').classList.contains('off') + '|' + document.querySelector('#allist .opt').getAttribute('aria-checked')
eval settings 'al15:' + (() => { const d = document.querySelector('#allist .danger'); d.click(); d.click(); return 'delete'; })()
wait 1500
eval settings 'al16:' + document.getElementById('allist').textContent
eval settings 'al18:' + (() => { const m = document.querySelector('main'); return m.scrollWidth <= m.clientWidth + 1; })()
eval settings 'al19:' + (() => { const d = document.getElementById('aldisconnect'); d.click(); d.click(); return 'disconnect'; })()
wait 800
eval settings 'al20:' + document.getElementById('alon').hidden
cmd {"cmd":"close_settings"}
wait 800
eval content 'al17:' + document.getElementById('e').value.length
cmd {"cmd":"set_language","language":"fr"}
cmd {"cmd":"open_settings","tab":"aliases"}
wait 1200
shot a07-aliases-fr
# ---- ads: the slots, the place they leave, and nothing else ----
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/ads"}
wait 4000
eval content 'ad1:' + ['s1', 'w1', 'w2', 'w3', 'w4', 'f', 'c'].map(i => document.getElementById(i).getClientRects().length ? 1 : 0).join('')
wait 5000
eval content 'ad2:' + (document.getElementById('w4').getClientRects().length ? 1 : 0)
cmd {"cmd":"open_settings","tab":"aliases"}
wait 1500
# ---- the button that opens the page of the key (last: the only step that needs the network) ----
eval settings 'kp1:' + (document.getElementById('alkeypage').click(), 'open')
wait 3000
eval content 'kp2:' + location.host + '|' + document.title
eval chrome 'alive:1'
quit
'@
$script = $script -replace '__PORT__', $port -replace '__KEY__', $key

$server = Start-Job -ArgumentList $port, $key, $apiLog -ScriptBlock {
    param($port, $key, $apiLog)
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $port)
    $listener.Start()
    $deadline = (Get-Date).AddMinutes(10)
    while ((Get-Date) -lt $deadline) {
        if (-not $listener.Pending()) { Start-Sleep -Milliseconds 20; continue }
        $client = $listener.AcceptTcpClient()
        $client.ReceiveTimeout = 3000
        try {
            $stream = $client.GetStream()
            $reader = [IO.StreamReader]::new($stream, [Text.Encoding]::ASCII, $false, 4096, $true)
            $request = $reader.ReadLine()
            $method, $path = ($request -split ' ')[0, 1]
            $auth = ''; $length = 0
            while ($line = $reader.ReadLine()) {
                if ($line -match '^Authentication: (.*)$') { $auth = $Matches[1] }
                if ($line -match '^Content-Length: (\d+)$') { $length = [int]$Matches[1] }
            }
            $posted = ''
            if ($length -gt 0) { $buf = New-Object char[] $length; [void]$reader.ReadBlock($buf, 0, $length); $posted = -join $buf }
            $status = '200 OK'; $type = 'text/html; charset=utf-8'
            if ($path -like '/api/*') {
                $type = 'application/json'
                Add-Content -Path $apiLog -Value "$method $path $posted" -Encoding ascii
                if ($auth -ne $key) { $status = '401 Unauthorized'; $body = '{"error":"Wrong api key"}' }
                elseif ($path -like '/api/user_info*') { $body = '{"name":"test"}' }
                elseif ($path -like '/api/alias/random/new*') { $status = '201 Created'; $body = '{"id":7,"email":"first.k7@alias.example","enabled":true}' }
                elseif ($path -like '*/toggle') { $body = '{"enabled":false}' }
                else { $body = '{"deleted":true}' }
            }
            elseif ($path -like '/favicon*') { $status = '404 Not Found'; $body = 'none' }
            elseif ($path -like '/ads*') {
                $body = '<title>bh ads</title><main><div id="w1" style="height:250px"><span>Advertisement</span><div data-ad-slot="true" id="s1" style="height:250px">x</div></div>' +
                    '<div id="w2" style="height:250px"><div class="ad-slot">x</div><p>A real paragraph of the article, kept.</p></div>' +
                    '<div id="w3" style="height:250px;width:600px"><span>Publicit&eacute;</span></div>' +
                    '<div id="w4" style="height:250px"><div class="ad-slot">x</div></div>' +
                    '<ul><li><span>Advertisement</span></li></ul><footer><span id="f">Advertisement</span></footer>' +
                    '<div class="ad-card" id="c">A classified ad</div></main>' +
                    '<script>document.adoptedStyleSheets=[];setTimeout(()=>{const i=document.createElement("button");i.textContent="Late";w4.append(i)},5000)</script>'
            }
            else {
                $body = '<title>bh signup</title><h1>Sign up</h1><form onsubmit="return false">' +
                    '<p><input id="e" type="email" name="email" style="width:300px;height:30px"></p>' +
                    '<p><input id="p" type="password" autocomplete="new-password"></p></form>'
            }
            $bytes = [Text.Encoding]::UTF8.GetBytes($body)
            $head = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 $status`r`nContent-Type: $type`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n")
            $stream.Write($head, 0, $head.Length)
            $stream.Write($bytes, 0, $bytes.Length)
        } catch {} finally { $client.Close() }
    }
    $listener.Stop()
}

# Real input in the page titled "bh go": a click in the email field, then Down and Enter.
$typist = Start-Job -ArgumentList $cdp -ScriptBlock {
    param($cdp)
    $deadline = (Get-Date).AddMinutes(9)
    $page = $null
    while (-not $page -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 200
        try { $page = (Invoke-RestMethod "http://127.0.0.1:$cdp/json") | Where-Object { $_.title -eq 'bh go' } | Select-Object -First 1 } catch {}
    }
    if (-not $page) { 'no page'; return }
    $none = [Threading.CancellationToken]::None
    $ws = [Net.WebSockets.ClientWebSocket]::new()
    $ws.ConnectAsync([Uri]$page.webSocketDebuggerUrl, $none).Wait()
    $script:id = 0
    function Send($method, $params) {
        $script:id++
        $json = @{ id = $script:id; method = $method; params = $params } | ConvertTo-Json -Compress -Depth 5
        $bytes = [Text.Encoding]::UTF8.GetBytes($json)
        $ws.SendAsync([ArraySegment[byte]]::new($bytes), 'Text', $true, $none).Wait()
        $buf = New-Object byte[] 65536
        $r = $ws.ReceiveAsync([ArraySegment[byte]]::new($buf), $none).Result
        [Text.Encoding]::UTF8.GetString($buf, 0, $r.Count)
    }
    $at = Send 'Runtime.evaluate' @{ expression = "(()=>{const r=document.getElementById('e').getBoundingClientRect();return (r.left+20)+','+(r.top+10)})()" } | ConvertFrom-Json
    $x, $y = $at.result.result.value -split ',' | ForEach-Object { [double]$_ }
    foreach ($type in 'mousePressed', 'mouseReleased') {
        [void](Send 'Input.dispatchMouseEvent' @{ type = $type; x = $x; y = $y; button = 'left'; clickCount = 1 })
    }
    Start-Sleep -Milliseconds 3500
    foreach ($k in @(@('ArrowDown', 40), @('Enter', 13))) {
        foreach ($type in 'keyDown', 'keyUp') {
            [void](Send 'Input.dispatchKeyEvent' @{ type = $type; key = $k[0]; code = $k[0]; windowsVirtualKeyCode = $k[1] })
        }
    }
    'typed'
}
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii

$logFile = Join-Path $tmp 'bh_run.log'
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=$cdp"
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(540000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
Write-Host "typist: $(Receive-Job $typist)"
foreach ($job in $server, $typist) { Stop-Job $job -ErrorAction SilentlyContinue; Remove-Job $job -Force -ErrorAction SilentlyContinue }
$log = Get-Content $logFile
$api = @(Get-Content $apiLog | Where-Object { $_ })
$all = @($log | Where-Object { $_ -match '\[dev\] eval ->' })
function Ev($name) { [string]($all | Where-Object { $_ -match "-> `"${name}:" } | Select-Object -First 1) }
$log | Where-Object { $_ -match '\[dev\] (bad|unknown|eval failed|screenshot failed)' } | ForEach-Object { Write-Host $_ }
$all | ForEach-Object { Write-Host "  $_" }
$api | ForEach-Object { Write-Host "  api $_" }

$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
$created = @($api | Where-Object { $_ -match 'random/new' })
Check ((Ev al1) -match 'al1:aliases\|false\|SimpleLogin,Addy\.io,Firefox Relay,Fastmail,Forward Email\|true') 'the section lists the services, no option while none is connected'
Check ($api.Count -gt 0 -and $api[0] -match 'GET /api/user_info') 'nothing was sent before the service was connected'
Check ((Ev al2) -match 'al2:The alias service refused the API key\|true') 'a wrong key is refused and not kept'
Check ((Ev al3) -match 'al3:Enter the https address') 'the key is never sent over plain http'
Check ((Ev al5) -match 'al5:SimpleLogin is connected\|false\|0\|false') 'connected, the key no longer in the page'
Check ((Ev al7) -match 'al7:"') 'a page posting the command itself (no menu token) gets nothing'
Check ((Ev al9) -match 'al9:first\.k7@alias\.example') 'the alias is written in the field'
Check ((Ev al10) -match 'al10:Alias created and kept for 127\.0\.0\.1:\d+Undo') 'a notice with Undo'
Check ($created.Count -eq 1 -and $created[0] -notmatch '127\.0\.0\.1') 'one alias made, the site not given to the service'
Check ((Ev al11) -match 'bank\.example=false,shop\.example=true') 'the password list marks the login that uses an alias'
Check ((Ev al12) -match 'al12:1\|.*first\.k7@alias\.example') 'the alias is listed with its site'
Check ((Ev al14) -match 'al14:true\|false') 'switched off'
Check ((Ev al16) -match 'al16:No alias yet' -and ($api -match 'DELETE /api/aliases/7').Count -eq 1) 'deleted, on the service too'
Check ((Ev al17) -match 'al17:0') 'the form is not left with the deleted address'
Check ((Ev al18) -match 'al18:true') 'no sideways scrolling'
Check ((Ev al20) -match 'al20:true') 'disconnected'
Check ((Ev mp1) -match 'mp1:none\|flex') 'no master password: only setting one is offered'
Check ((Ev ad1) -match 'ad1:0010011') 'ads: slots, wrappers and lone labels hidden (the page replaced its styles); text, footer and classifieds kept'
Check ((Ev ad2) -match 'ad2:1') 'ads: a wrapper comes back when real content arrives in it'
Check ((Ev kp2) -match 'kp2:app\.simplelogin\.io') 'Find my key opens the page of the service'
Check ((Ev alive) -match 'alive:1') 'no crash: the app is still running'
if ($fail) { exit 1 }
