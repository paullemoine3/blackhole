# 0.8.31 scenarios on Windows, driven by the devtools harness (see crates/app/src/devtools.rs):
# site permissions (prompt, answers kept, blocked, dismissed, black hole tab, Settings and shield
# menu), erasing data (cookies, local storage, history), search keywords, downloads paused,
# resumed and cancelled, Settings > Import and export.
# Uses the scratch profile %TEMP%\bh_data (wiped first); screenshots land in %TEMP%\bh_shots.
# No network. Local servers on 47992 (pages) and 47993 (a slow file with ranges).
# Every eval is named ("pm1:"...) and read by name.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue
$port = 47992
$slow = 47993
$downloads = (New-Object -ComObject Shell.Application).NameSpace('shell:Downloads').Self.Path
$name = 'bh-test-0831'
Remove-Item (Join-Path $downloads "$name*") -ErrorAction SilentlyContinue

$script = @'
wait 3000
cmd {"cmd":"set_language","language":"en"}
# ---- a page asks: the prompt, nothing answered while it is fresh, then Always allow ----
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/a"}
wait 2500
eval content 'pm0:' + (window.r = 'waiting', Notification.requestPermission().then(x => r = x), 'asked')
wait 350
eval palette 'pm1:' + document.body.classList.contains('permitting') + '|' + document.getElementById('ptitle').textContent + '|' + [...document.querySelectorAll('#pwanted span')].map(x => x.textContent).join(',') + '|' + document.activeElement.id + '|' + (document.getElementById('pallow').click(), 'early')
wait 700
eval content 'pm2:' + window.r
shot p01-prompt
eval palette 'pm3:' + (document.getElementById('pallow').click(), 'allow')
wait 900
eval content 'pm4:' + window.r
# ---- remembered: a reload asks nothing ----
cmd {"cmd":"reload"}
wait 2000
eval content 'pm5:' + (window.r = 'waiting', Notification.requestPermission().then(x => r = x), 'asked')
wait 900
eval content 'pm6:' + window.r
# ---- the shield's menu says so and blocks it ----
cmd {"cmd":"privacy_report","x":300,"y":10}
wait 900
shot p02-shield-menu
eval palette 'pm7:' + document.body.textContent.includes('Notifications \u00b7 allowed')
cmd {"cmd":"palette_close"}
cmd {"cmd":"permission_set","origin":"http://127.0.0.1:__PORT__","permission":"notifications","allowed":false}
cmd {"cmd":"reload"}
wait 2000
eval content 'pm8:' + (window.r = 'waiting', Notification.requestPermission().then(x => r = x), 'asked')
wait 900
eval content 'pm9:' + window.r
# ---- the button at the left of the address ----
eval bar 'sb1:' + document.getElementById('site').hidden + '|' + document.getElementById('site').classList.contains('set') + '|' + (document.getElementById('site').click(), 'open')
wait 900
shot p06-site-panel
eval palette 'sb2:' + [...document.querySelectorAll('.mchoice')].map(r => r.querySelector('.nm').textContent + '=' + r.querySelector('[aria-checked=true]').textContent).join(',')
eval palette 'sb3:' + (document.querySelectorAll('.mchoice')[0].querySelectorAll('button')[1].click(), 'allow camera')
wait 500
eval palette 'sb4:' + document.body.classList.contains('menuing') + '|' + document.querySelectorAll('.mchoice')[0].querySelector('[aria-checked=true]').textContent
cmd {"cmd":"palette_close"}
cmd {"cmd":"set_language","language":"fr"}
wait 500
eval bar 'sb6:' + (document.getElementById('site').click(), 'fr')
wait 900
eval palette 'sb7:' + [...document.querySelectorAll('.mchoice .seg button')].every(b => b.offsetHeight < 32)
shot p07-site-panel-fr
cmd {"cmd":"set_language","language":"en"}
cmd {"cmd":"palette_close"}
cmd {"cmd":"home"}
wait 700
eval bar 'sb5:' + document.getElementById('site').hidden
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/a"}
wait 1500
# ---- location: Esc refuses without keeping it; three times, the site is not heard any more ----
eval content 'pm10:' + (window.g = [], window.ask = () => navigator.geolocation.getCurrentPosition(() => g.push('ok'), e => g.push(e.code)), ask(), 'asked')
wait 900
cmd {"cmd":"palette_close"}
wait 600
eval content 'pm11:' + (ask(), g.join())
wait 900
cmd {"cmd":"palette_close"}
wait 600
eval content 'pm12:' + (ask(), g.join())
wait 900
cmd {"cmd":"palette_close"}
wait 600
eval content 'pm13:' + (ask(), g.join())
wait 900
eval palette 'pm14:' + document.body.classList.contains('permitting')
eval content 'pm15:' + g.join()
cmd {"cmd":"permission_forget","origin":"http://127.0.0.1:__PORT__"}
# ---- a frame of another origin the page lets ask: like Chromium, WebView2 asks for the page ----
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/frame"}
wait 3500
eval palette 'pm16:' + document.body.classList.contains('permitting') + '|' + document.getElementById('ptitle').textContent
shot p03-frame
cmd {"cmd":"palette_close"}
wait 500
# ---- a page that leaves takes its question with it ----
cmd {"cmd":"permission_forget","origin":"http://127.0.0.1:__PORT__"}
eval content 'pm17:' + (Notification.requestPermission(), 'asked')
wait 900
eval content 'pm18:' + (location.href = '/b', 'left')
wait 2500
eval palette 'pm19:' + document.body.classList.contains('permitting')
# ---- Settings > Privacy ----
cmd {"cmd":"permission_set","origin":"https://meet.example","permission":"camera","allowed":true}
cmd {"cmd":"permission_set","origin":"https://meet.example","permission":"microphone","allowed":true}
cmd {"cmd":"permission_set","origin":"https://ads.example","permission":"location","allowed":false}
cmd {"cmd":"open_settings","tab":"privacy"}
wait 1800
eval settings 'ps1:' + document.querySelectorAll('#permlist .row').length + '|' + document.getElementById('permlist').textContent
eval settings 'ps2:' + (document.getElementById('permlist').scrollIntoView({block: 'center'}), 'scroll')
wait 600
shot p04-settings-permissions
eval settings 'ps3:' + (document.getElementById('clkinds').scrollIntoView({block: 'center'}), 'scroll')
wait 600
shot p05-settings-clear
eval settings 'ps4:' + (() => { const m = document.querySelector('main'); return m.scrollWidth <= m.clientWidth + 1; })()
cmd {"cmd":"close_settings"}
# ---- erasing: cookies and local storage of the space, history ----
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/c"}
wait 2000
eval content 'cl1:' + (document.cookie = 'bh=1; max-age=9999', localStorage.setItem('bh', '1'), document.cookie + '|' + localStorage.getItem('bh'))
cmd {"cmd":"clear_data","kinds":["cookies","cache","history"],"period":"hour"}
wait 4000
eval chrome 'cl2:' + document.getElementById('toast').textContent
cmd {"cmd":"reload"}
wait 2000
eval content 'cl3:' + document.cookie + '|' + localStorage.getItem('bh')
cmd {"cmd":"new_space","name":"Second","color":"#ff5c8a"}
wait 1500
cmd {"cmd":"clear_data","kinds":["cookies"],"period":"all","all_spaces":true}
wait 5000
eval chrome 'cl4:' + document.getElementById('toast').textContent
# ---- keywords ----
cmd {"cmd":"keyword_add","keyword":"bh","name":"Local","url":"http://127.0.0.1:__PORT__/search?q=%s"}
cmd {"cmd":"palette","mode":"new"}
wait 800
eval palette 'kw0:' + (document.getElementById('q').value = 'bh two words', document.getElementById('q').dispatchEvent(new InputEvent('input')), 'typed')
wait 700
eval palette 'kw1:' + document.getElementById('list').textContent
shot k01-keyword
cmd {"cmd":"open","input":"bh two words"}
wait 2500
eval content 'kw2:' + location.pathname + location.search
cmd {"cmd":"open","input":"more !bh"}
wait 2500
eval content 'kw3:' + location.pathname + location.search
cmd {"cmd":"open_settings","tab":"general"}
wait 1500
eval settings 'kw4:' + (document.getElementById('kwkey').value = 'a b', document.getElementById('kwurl').value = 'https://x.test/%s', document.getElementById('kwadd').click(), 'bad')
wait 600
eval settings 'kw5:' + document.getElementById('kwerr').textContent + '|' + document.querySelectorAll('#kwlist .row').length
eval settings 'kw6:' + (document.getElementById('kwlist').scrollIntoView({block: 'center'}), 'scroll')
wait 500
shot k02-settings-keywords
cmd {"cmd":"open_settings","tab":"import"}
wait 1200
eval settings 'bk1:' + ['exp-html', 'exp-csv', 'bksave', 'bkrestore'].map(i => document.getElementById(i).disabled).join(',') + '|' + document.getElementById('bkready').hidden
eval settings 'bk2:' + (document.getElementById('bkpass').value = 'one', document.getElementById('bkpass2').value = 'two', document.getElementById('bksave').click(), document.getElementById('bkstatus').textContent)
eval settings 'bk3:' + (document.getElementById('bkpass').value = document.getElementById('bkpass2').value = 'short', document.getElementById('bksave').click(), 'short')
wait 1500
eval settings 'bk4:' + document.getElementById('bkstatus').textContent + '|' + document.getElementById('bkpass').value.length
eval settings 'bk5:' + (document.getElementById('bksave').scrollIntoView({block: 'center'}), 'scroll')
wait 500
shot b01-backup
cmd {"cmd":"set_language","language":"fr"}
wait 800
shot b02-backup-fr
cmd {"cmd":"set_language","language":"en"}
cmd {"cmd":"close_settings"}
# ---- downloads ----
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/dl"}
wait 2000
eval content 'dl0:' + (document.querySelector('a').click(), 'started')
wait 2500
eval chrome 'dl1:' + document.querySelectorAll('.dl .act.on').length + '|' + document.querySelector('.dl .pc').textContent
eval chrome 'dl2:' + (document.querySelector('.dl .act.on').click(), 'pause')
wait 1500
eval chrome 'dl3:' + document.querySelector('.dl').className + '|' + document.querySelector('.dl .pc').textContent + '|' + document.querySelector('.dl .act.on').dataset.act + '|' + document.querySelector('.dl').title
shot d01-paused
wait 3000
eval chrome 'dl4:' + document.querySelector('.dl').title
cmd {"cmd":"open_library","section":"downloads"}
wait 1200
eval library 'dl5:' + document.body.textContent.includes('Paused')
shot d02-library
cmd {"cmd":"close_library"}
eval chrome 'dl6:' + (document.querySelector('.dl .act.on').click(), 'resume')
wait 1500
eval chrome 'dl7:' + document.querySelector('.dl').className + '|' + document.querySelector('.dl').title
wait 5500
eval chrome 'dl8:' + [...document.querySelectorAll('.dl')].map(x => x.className).join()
cmd {"cmd":"open","input":"http://localhost:__PORT__/dl2"}
wait 2000
eval content 'dl9:' + (document.querySelector('a').click(), 'again')
wait 2000
eval chrome 'dl10:' + (document.querySelector('.dl:not(.ok) .act.on ~ .act.on').click(), 'cancel')
wait 1500
eval chrome 'dl11:' + document.querySelectorAll('.dl:not(.ok):not(.out)').length + '|' + document.getElementById('toast').textContent
eval chrome 'alive:1'
quit
'@
$script = $script -replace '__PORT__', $port

$pages = Start-Job -ArgumentList $port, $slow, $name -ScriptBlock {
    param($port, $slow, $name)
    $listener = [Net.Sockets.TcpListener]::Create($port)
    $listener.Start()
    $deadline = (Get-Date).AddMinutes(10)
    while ((Get-Date) -lt $deadline) {
        if (-not $listener.Pending()) { Start-Sleep -Milliseconds 20; continue }
        $client = $listener.AcceptTcpClient()
        $client.ReceiveTimeout = 2000
        try {
            $stream = $client.GetStream()
            $reader = [IO.StreamReader]::new($stream, [Text.Encoding]::ASCII, $false, 4096, $true)
            $path = ($reader.ReadLine() -split ' ')[1]
            while ($reader.ReadLine()) {}
            $body = switch -Wildcard ($path) {
                '/frame' { "<title>bh frame</title><iframe allow='geolocation' src='http://localhost:$port/inner'></iframe>" }
                '/inner' { '<script>setTimeout(()=>navigator.geolocation.getCurrentPosition(()=>{},()=>{}),500)</script>' }
                '/dl*' { "<title>bh dl</title><a href='http://127.0.0.1:$slow/$name.bin' download>file</a>" }
                default { "<title>bh $path</title><h1>$path</h1>" }
            }
            $bytes = [Text.Encoding]::UTF8.GetBytes($body)
            $head = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 200 OK`r`nContent-Type: text/html; charset=utf-8`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n")
            $stream.Write($head, 0, $head.Length)
            $stream.Write($bytes, 0, $bytes.Length)
        } catch {} finally { $client.Close() }
    }
}

# 6 MB in about ten seconds per client (one thread each); a request with a range goes on from there.
$file = Start-Job -ArgumentList $slow -ScriptBlock {
    param($port)
    Add-Type -TypeDefinition @"
using System; using System.IO; using System.Net; using System.Net.Sockets; using System.Text; using System.Threading;
public static class SlowFile {
  const int Size = 6 * 1024 * 1024;
  public static System.Collections.Concurrent.ConcurrentQueue<string> Log = new System.Collections.Concurrent.ConcurrentQueue<string>();
  public static void Start(int port) {
    var l = new TcpListener(IPAddress.Loopback, port); l.Start();
    new Thread(() => { while (true) { var c = l.AcceptTcpClient(); new Thread(() => Serve(c)) { IsBackground = true }.Start(); } }) { IsBackground = true }.Start();
  }
  static void Serve(TcpClient c) {
    try {
      c.SendBufferSize = 16384; c.NoDelay = true;
      var s = c.GetStream(); var r = new StreamReader(s, Encoding.ASCII, false, 4096, true);
      string line, request = r.ReadLine(); int from = 0;
      while (!string.IsNullOrEmpty(line = r.ReadLine()))
        if (line.StartsWith("Range: bytes=")) from = int.Parse(line.Substring(13).Split('-')[0]);
      Log.Enqueue(request + " from=" + from);
      var status = from > 0 ? "206 Partial Content\r\nContent-Range: bytes " + from + "-" + (Size - 1) + "/" + Size : "200 OK";
      var head = Encoding.ASCII.GetBytes("HTTP/1.1 " + status + "\r\nContent-Type: application/octet-stream\r\nAccept-Ranges: bytes\r\nETag: \"bh1\"\r\nLast-Modified: Wed, 01 Jul 2026 10:00:00 GMT\r\nContent-Length: " + (Size - from) + "\r\nConnection: close\r\n\r\n");
      s.Write(head, 0, head.Length);
      var buf = new byte[30000];
      for (int at = from; at < Size; at += buf.Length) {
        int n = Math.Min(buf.Length, Size - at);
        for (int k = 0; k < n; k++) buf[k] = (byte)((at + k) % 251);
        s.Write(buf, 0, n); Thread.Sleep(45);
      }
      c.Client.Shutdown(SocketShutdown.Send); Thread.Sleep(300);
    } catch (Exception e) { Log.Enqueue("end " + e.GetType().Name); } finally { c.Close(); }
  }
}
"@
    [SlowFile]::Start($port)
    $line = ''
    while ($true) { while ([SlowFile]::Log.TryDequeue([ref]$line)) { $line }; Start-Sleep -Milliseconds 200 }
}
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii

$logFile = Join-Path $tmp 'bh_run.log'
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(540000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
$served = @(Receive-Job $file -ErrorAction SilentlyContinue)
foreach ($job in $pages, $file) { Stop-Job $job -ErrorAction SilentlyContinue; Remove-Job $job -Force -ErrorAction SilentlyContinue }
$log = Get-Content $logFile -Encoding utf8
$all = @($log | Where-Object { $_ -match '\[dev\] eval ->' })
function Ev($name) { [string]($all | Where-Object { $_ -match "-> `"${name}:" } | Select-Object -First 1) }
$log | Where-Object { $_ -match '\[dev\] (bad|unknown|eval failed|screenshot failed)|panicked' } | ForEach-Object { Write-Host $_ }
$all | ForEach-Object { Write-Host "  $_" }
$served | ForEach-Object { Write-Host "  file $_" }

$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
Check ((Ev pm1) -match "pm1:true\|127\.0\.0\.1:$port wants to use\|Notifications\|perm\|early") 'a page asks: the prompt names the site and what it wants, no button focused'
Check ((Ev pm2) -match 'pm2:waiting') 'a click in the first half second answers nothing'
Check ((Ev pm4) -match 'pm4:granted') 'Always allow grants'
Check ((Ev pm6) -match 'pm6:granted') 'and is remembered: no question after a reload'
Check ((Ev pm7) -match 'pm7:true') "the shield's menu lists it"
Check ((Ev pm9) -match 'pm9:denied') 'blocked from there, the site is refused without a question'
Check ((Ev sb1) -match 'sb1:false\|true') 'the button of the address shows on a web page, with its dot'
Check ((Ev sb2) -match 'Camera=Ask,Microphone=Ask,Your location=Ask,Notifications=Block,Clipboard content=Ask') 'its panel lists every permission with what was decided'
Check ((Ev sb4) -match 'sb4:true\|Allow') 'a choice moves and the panel stays'
Check ((Ev sb7) -match 'sb7:true') 'the choices keep to one line in French'
Check ((Ev sb5) -match 'sb5:true') 'no button on the start page'
Check ((Ev pm13) -match 'pm13:1,1') 'closing the prompt refuses'
Check ((Ev pm14) -match 'pm14:false') 'after three dismissals the site no longer asks'
Check ((Ev pm15) -match 'pm15:1,1,1,1') 'and every request was refused'
Check ((Ev pm16) -match "pm16:true\|127\.0\.0\.1:$port wants") 'a frame of another origin asks in the name of the page'
Check ((Ev pm19) -match 'pm19:false') 'leaving the page closes its question'
Check ((Ev ps1) -match 'ps1:2\|.*ads\.example.*Location|ps1:2\|.*ads\.example.*location') 'Settings list the sites and what they may use'
Check ((Ev ps4) -match 'ps4:true') 'no horizontal scroll'
Check ((Ev cl1) -match 'cl1:bh=1\|1') 'a cookie and local storage are there'
Check ((Ev cl2) -match 'cl2:Browsing data erased') 'erasing says it is done'
Check ((Ev cl3) -match 'cl3:\|null') 'and they are gone'
Check ((Ev cl4) -match 'cl4:Browsing data erased') 'every space, including one with no page open'
Check ((Ev kw1) -match 'two words.*Local') 'the command bar names the engine of the keyword'
Check ((Ev kw2) -match 'kw2:/search\?q=two%20words') 'the keyword searches its engine'
Check ((Ev kw3) -match 'kw3:/search\?q=more') 'a bang at the end too'
Check ((Ev kw5) -match 'kw5:The keyword is one word.*\|7') 'a bad keyword is refused, the list kept'
Check ((Ev bk1) -match 'bk1:false,false,false,false\|true') 'export and backup are offered'
Check ((Ev bk2) -match 'bk2:The two passwords differ') 'the two passwords must match'
Check ((Ev bk4) -match 'bk4:The backup password needs at least 8 characters\|0') 'a short password is refused and not kept in the page'
Check ((Ev dl1) -match 'dl1:2\|') 'a download offers pause and cancel'
Check ((Ev dl3) -match 'dl3:dl paused\|Paused\|resume') 'paused'
Check ((Ev dl3) -match 'MB|KB' -and ((Ev dl3) -replace '.*resume\|', '') -eq ((Ev dl4) -replace '.*dl4:', '')) 'nothing is received while paused'
Check ((Ev dl5) -match 'dl5:true') 'the Library says so'
Check ((Ev dl7) -match 'dl7:dl( unk)?\|') 'resumed'
Check ((Ev dl8) -match 'dl ok') 'the download ends well'
Check ((Ev dl10) -match 'cancel' -and @($served | Where-Object { $_ -match '^GET' }).Count -ge 2 -and (Ev dl11) -match 'dl11:0\|' -and (Ev dl11) -notmatch 'failed') 'a cancelled download leaves, without a failure'
$got = Join-Path $downloads "$name.bin"
$whole = (Test-Path $got) -and @([IO.File]::ReadAllBytes($got) | Select-Object -Skip 3000000 -First 5000 | ForEach-Object -Begin { $k = 3000000 } -Process { if ($_ -ne ($k % 251)) { 'bad' }; $k++ }).Count -eq 0
Check ($whole -and (Get-Item $got).Length -eq 6MB) 'the file is whole'
Check (-not (Test-Path (Join-Path $downloads "$name (2).bin"))) 'the cancelled one is not left behind'
Check ((Ev alive) -match 'alive:1') 'the app is alive at the end'
Remove-Item (Join-Path $downloads "$name*") -ErrorAction SilentlyContinue
if ($fail) { exit 1 }
