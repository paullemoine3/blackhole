# 0.8.25 scenarios on Windows, driven by the devtools harness (see crates/app/src/devtools.rs):
# split view from a quick access tile and no tile size in the menus, "Copy image" on a picture
# inside a link (the system clipboard holds an image at the end), "Save image" (a PNG served under
# a .php address lands in Downloads as bhsave0825.png, removed at the end), Ctrl+Shift+C, the loading line
# drawn at the top of the page (on the page left while a slow address waits, then gone), the
# black hole when the Settings open (mid-transition, and nothing with animations off), the
# "Add an extension" drop-down (GitHub or a link, a refused address), the "What syncs" card.
# Uses the scratch profile %TEMP%\bh_data (wiped first); screenshots land in %TEMP%\bh_shots.
# No network: a local server on 47986. Every eval is named ("qa1:", "cp2:"…) and read by name.
# Not covered here: YouTube ads (network), Twitch Picture in Picture, a sync between two devices.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue
$port = 47986

$script = @'
wait 3000
cmd {"cmd":"set_language","language":"en"}
# ---- quick access: split view from a tile, no tile size in the menus ----
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/other"}
wait 2500
eval chrome 'qa1:' + (() => { let got = []; const keep = openMenu; openMenu = (x, y, items) => { got = items; }; try { const t = Object.assign({}, state.tabs[0], { id: 987654, pinned: true, split: false, asleep: true }); tabMenu(t, 0, 0); const tile = got.map(i => i.head || (i.run && i.run.cmd) || '').join(','); createMenu(0, 0); const area = got.map(i => i.head || (i.run && i.run.cmd) || '').join(','); return /(^|,)split(,|$)/.test(tile) + '|' + /set_quick_access_size/.test(tile + area) + '|' + /Tile size/.test(tile + area); } finally { openMenu = keep; } })()
# ---- copy: Ctrl+Shift+C, then "Copy image" on a picture inside a link ----
cmd {"cmd":"open","input":"http://127.0.0.1:__PORT__/pic"}
wait 2500
eval content 'cp0:' + (document.dispatchEvent(new KeyboardEvent('keydown', { key: 'C', code: 'KeyC', ctrlKey: true, shiftKey: true, bubbles: true, cancelable: true })), 'sent')
wait 900
eval chrome 'cp1:' + document.getElementById('toast').textContent
eval content 'cp2:' + (() => { const img = document.getElementById('im'); const r = img.getBoundingClientRect(); img.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + 5, clientY: r.top + 5 })); return 'asked'; })()
wait 1200
eval palette 'cp3:' + [...document.querySelectorAll('#menu button')].map(b => b.textContent).filter(t => /image|link/i.test(t)).join(',')
shot c01-image-menu
eval palette 'cp4:' + (() => { const b = [...document.querySelectorAll('#menu button')].find(b => b.textContent === 'Copy image'); if (!b) return 'missing'; b.click(); return 'clicked'; })()
wait 2500
eval chrome 'cp5:' + document.getElementById('toast').textContent
eval content 'cp6:' + (() => { const img = document.getElementById('im'); const r = img.getBoundingClientRect(); img.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + 5, clientY: r.top + 5 })); return 'asked'; })()
wait 1200
eval palette 'cp7:' + (() => { const b = [...document.querySelectorAll('#menu button')].find(b => b.textContent === 'Save image'); if (!b) return 'missing'; b.click(); return 'clicked'; })()
wait 2500
eval chrome 'cp8:' + document.getElementById('toast').textContent
# ---- the loading line at the top of the page ----
cmd {"cmd":"navigate","input":"http://127.0.0.1:__PORT__/slow"}
wait 700
eval bar 'pg1:' + document.getElementById('progress').classList.contains('on') + '|' + document.getElementById('url').textContent
shot c02-waiting-line
wait 1800
eval content 'pg4:' + getComputedStyle(document.documentElement, '::before').content + '|' + (parseFloat(getComputedStyle(document.documentElement, '::before').width) > 0) + '|' + location.pathname
eval bar 'pg5:' + document.getElementById('progress').classList.contains('on')
shot c02b-page-line
wait 4500
eval content 'pg2:' + getComputedStyle(document.documentElement, '::before').content + '|' + location.pathname
eval bar 'pg3:' + document.getElementById('progress').classList.contains('on')
# ---- Settings: the black hole at the opening (a first opening creates the page) ----
cmd {"cmd":"open_settings","tab":"extensions"}
wait 2500
cmd {"cmd":"close_settings"}
wait 500
cmd {"cmd":"open_settings","tab":"extensions"}
wait 350
eval settings 'st1:' + document.getElementById('sx-warp').classList.contains('go') + '|' + document.body.classList.contains('sx-emerge')
shot c03-settings-hole
wait 1500
eval settings 'st2:' + document.getElementById('sx-warp').classList.contains('go') + '|' + document.body.classList.contains('sx-emerge')
shot c04-settings
# ---- "Add an extension": GitHub or a link ----
eval settings 'mk1:' + (document.getElementById('mkgh').click(), !document.getElementById('mkaddmenu').hidden + '|' + document.querySelectorAll('#mkaddmenu [data-add]').length)
shot c05-add-menu
eval settings 'mk2:' + (document.querySelector('#mkaddmenu [data-add="link"]').click(), document.getElementById('mkaddmenu').hidden + '|' + document.getElementById('ghrepo').placeholder + '|' + document.querySelector('#mkgithub .title').textContent)
shot c06-add-link
eval settings 'mk3:' + (document.getElementById('ghrepo').value = 'http://example.com/a.crx', document.getElementById('ghrepo').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })), 'sent')
wait 800
eval settings 'mk4:' + (document.querySelector('#mkgithub .fielderr') || {}).textContent
shot c07-add-link-refused
eval settings 'mk5:' + (ghClose(), 'closed')
wait 500
# ---- Settings > Devices: what syncs ----
eval settings 'sy1:' + (show('devices'), renderDevices({ available: true, code: null, status: '', list: [{ id: 'a', name: 'Studio Mac', online: true, sync: 'ask' }], history: { on: true, last: null }, extensions: { on: true, host: true, last: { name: 'Studio Mac', at: 1700000000 } } }), [...document.querySelectorAll('#histsync .label')].map(l => l.textContent).join(',') + '|' + document.getElementById('histcard').hidden + '|' + document.querySelector('#histcard h2').textContent)
shot c08-what-syncs
eval settings 'sy2:' + (renderDevices({ available: true, code: null, status: '', list: [{ id: 'a', name: 'Studio Mac', online: true, sync: 'ask' }], history: { on: true }, extensions: { on: false, host: false } }), document.querySelectorAll('#histsync .label').length)
# ---- animations off: the Settings open without the hole ----
cmd {"cmd":"close_settings"}
wait 400
cmd {"cmd":"set_animations","on":false}
wait 300
cmd {"cmd":"open_settings"}
wait 350
eval settings 'st3:' + document.getElementById('sx-warp').classList.contains('go')
cmd {"cmd":"set_animations","on":true}
cmd {"cmd":"close_settings"}
quit
'@
$script = $script -replace '__PORT__', $port

# Local pages: /pic (a picture inside a link), /img.png, /slow (answers after 3 s), /other.
$server = Start-Job -ArgumentList $port -ScriptBlock {
    param($port)
    Add-Type -AssemblyName System.Drawing
    $bmp = [Drawing.Bitmap]::new(8, 8)
    for ($x = 0; $x -lt 8; $x++) { for ($y = 0; $y -lt 8; $y++) { $bmp.SetPixel($x, $y, [Drawing.Color]::FromArgb(255, 32 * $x, 32 * $y, 200)) } }
    $ms = [IO.MemoryStream]::new()
    $bmp.Save($ms, [Drawing.Imaging.ImageFormat]::Png)
    $png = $ms.ToArray()
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
            $type = 'text/html; charset=utf-8'; $status = '200 OK'
            if ($path -like '/img.png*' -or $path -like '/bhsave0825*') { $bytes = $png; $type = 'image/png' }
            else {
                if ($path -like '/favicon*') { $status = '404 Not Found'; $body = 'none' }
                elseif ($path -like '/pic*') { $body = '<title>bh pic</title><a href="/other"><img id="im" src="/bhsave0825.php" width="200" height="200" style="image-rendering:pixelated"></a>' }
                elseif ($path -like '/slow*') {
                    # The server waits, then the page starts and keeps loading for a while.
                    Start-Sleep -Milliseconds 1500
                    $first = [Text.Encoding]::UTF8.GetBytes('<title>bh slow</title><h1>Slow</h1>' + (' ' * 2048))
                    $rest = [Text.Encoding]::UTF8.GetBytes('<p>end</p>')
                    $head = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 200 OK`r`nContent-Type: text/html; charset=utf-8`r`nContent-Length: $($first.Length + $rest.Length)`r`nConnection: close`r`n`r`n")
                    $stream.Write($head, 0, $head.Length); $stream.Write($first, 0, $first.Length); $stream.Flush()
                    Start-Sleep -Milliseconds 2500
                    $stream.Write($rest, 0, $rest.Length)
                    continue
                }
                else { $body = '<title>bh other</title><h1>Other</h1>' }
                $bytes = [Text.Encoding]::UTF8.GetBytes($body)
            }
            $head = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 $status`r`nContent-Type: $type`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n`r`n")
            $stream.Write($head, 0, $head.Length)
            $stream.Write($bytes, 0, $bytes.Length)
        } catch {} finally { $client.Close() }
    }
    $listener.Stop()
}
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii

$downloads = (New-Object -ComObject Shell.Application).NameSpace('shell:Downloads').Self.Path
Remove-Item (Join-Path $downloads 'bhsave0825*') -ErrorAction SilentlyContinue
$logFile = Join-Path $tmp 'bh_run.log'
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(540000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
Stop-Job $server -ErrorAction SilentlyContinue; Remove-Job $server -Force -ErrorAction SilentlyContinue
$log = Get-Content $logFile
$all = @($log | Where-Object { $_ -match '\[dev\] eval ->' })
function Ev($name) { [string]($all | Where-Object { $_ -match "-> `"${name}:" } | Select-Object -First 1) }
$log | Where-Object { $_ -match '\[dev\] (bad|unknown|eval failed|screenshot failed)' } | ForEach-Object { Write-Host $_ }
Add-Type -AssemblyName System.Windows.Forms
$clipImage = [Windows.Forms.Clipboard]::ContainsImage()
$saved = Join-Path $downloads 'bhsave0825.png'
$savedOk = (Test-Path $saved) -and ((Get-Content $saved -Encoding Byte -TotalCount 4) -join ',') -eq '137,80,78,71'
Remove-Item (Join-Path $downloads 'bhsave0825*') -ErrorAction SilentlyContinue

$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
Check ((Ev qa1) -match 'qa1:true\|false\|false"') 'quick access tile: split view offered, no tile size in the tile or area menus'
Check ((Ev cp1) -match 'cp1:.*Link copied') 'Ctrl+Shift+C copies the address of the page'
Check ((Ev cp3) -match 'Copy image' -and (Ev cp3) -match 'Copy link') 'a picture inside a link: the link entries and the picture ones'
Check ((Ev cp5) -match 'cp5:.*Image copied') 'Copy image: the shell downloads it and puts it on the clipboard'
Check $clipImage 'the system clipboard holds an image'
Check ((Ev cp7) -match 'cp7:clicked' -and (Ev cp8) -match 'cp8:.*Image saved in Downloads') 'Save image: offered beside Copy image, then saved'
Check $savedOk 'the file is in Downloads, named after its address with its real format (.png, not .php)'
Check ((Ev pg1) -match 'pg1:true\|127.0.0.1:\d+/slow"') 'a typed address waiting for its server: the line under the top bar, the address shown'
Check ((Ev pg4) -match 'pg4:.+\|true\|/slow"') 'the page started: the loading line runs at the top of the page itself'
Check ((Ev pg5) -match 'pg5:false"') 'and the top bar line has handed over'
Check ((Ev pg2) -match 'pg2:none\|/slow"') 'loaded: the line is gone and its sheet removed'
Check ((Ev pg3) -match 'pg3:false"') 'no line in the top bar once the page is there'
Check ((Ev st1) -match 'st1:true\|true"') 'Settings opened: the black hole runs (mid-transition)'
Check ((Ev st2) -match 'st2:false\|false"') 'then the Settings are back to rest'
Check ((Ev mk1) -match 'mk1:true\|2"') 'Add an extension drops its menu: GitHub, a link'
Check ((Ev mk2) -match 'mk2:true\|https://.+\|Add from a link"') 'From a link: an address field'
Check ((Ev mk4) -match 'mk4:Type the https address') 'a plain http address is refused'
Check ((Ev sy1) -match 'sy1:Tab sync,Share the history with paired devices,Share the extensions with paired devices\|false\|What syncs"') 'Devices: one card says what syncs (tabs, history, extensions)'
Check ((Ev sy2) -match 'sy2:2"') 'no extension host: no extensions line'
Check ((Ev st3) -match 'st3:false"') 'animations off: no black hole'
Write-Host "Screenshots: $(Join-Path $tmp 'bh_shots')"
if ($fail) { exit 1 }
