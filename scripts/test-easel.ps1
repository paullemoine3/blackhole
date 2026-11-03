# Easel scenarios on Windows, driven by the devtools harness (see crates/app/src/devtools.rs):
# create, capture a page, notes, edits, link back to the page, rename, delete, live capture (pick an
# area, play it, check the crop), persistence.
# Uses the scratch profile %TEMP%\bh_data (wiped first); screenshots land in %TEMP%\bh_shots.
# Needs network access and a visible, uncovered desktop (the window is captured from the screen).
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue

$script = @'
wait 3000
cmd {"cmd":"open","input":"https://example.com"}
wait 5000
cmd {"cmd":"new_easel","name":"Ideas"}
cmd {"cmd":"open_easel"}
wait 2000
eval easel document.getElementById('title').value
cmd {"cmd":"capture_to_easel"}
wait 1500
eval easel JSON.stringify([...document.querySelectorAll('.card.clip .link')].map(a => a.textContent))
eval easel document.querySelector('.card.clip .host').textContent
cmd {"cmd":"easel_add_note"}
wait 500
eval easel document.querySelectorAll('.card').length
eval easel (() => { const t = document.querySelector('.card.note textarea'); t.value = 'hello easel'; t.dispatchEvent(new Event('input')); return 'typed'; })()
wait 600
cmd {"cmd":"easel_move","item":2,"x":420,"y":300}
cmd {"cmd":"easel_color","item":2,"color":"#2e8b6e"}
wait 500
shote 01-easel-board
cmd {"cmd":"home"}
wait 800
cmd {"cmd":"capture_to_easel"}
wait 800
eval easel document.querySelectorAll('.card').length
cmd {"cmd":"open","input":"https://example.org"}
wait 4000
cmd {"cmd":"easel_open_link","item":1}
wait 4000
eval content location.href
cmd {"cmd":"rename_easel","id":1,"name":"Renamed"}
cmd {"cmd":"new_easel","name":"Second"}
wait 500
eval easel JSON.stringify([...document.querySelectorAll('.easel .name')].map(n => n.textContent))
shote 02-easel-two-boards
cmd {"cmd":"delete_easel","id":2}
wait 500
eval easel document.querySelectorAll('.easel').length
cmd {"cmd":"easel_remove","item":2}
wait 500
eval easel document.querySelectorAll('.card').length
cmd {"cmd":"open","input":"https://example.com"}
wait 4000
cmd {"cmd":"live_pick"}
wait 500
eval content String(window.__bhPick === true)
eval content (() => { const h = document.querySelector('p').getBoundingClientRect(); const x = h.left + h.width / 2, y = h.top + h.height / 2; for (const t of ['mousemove', 'click']) document.elementFromPoint(x, y).dispatchEvent(new MouseEvent(t, { bubbles: true, cancelable: true, clientX: x, clientY: y, view: window })); return 'picked'; })()
wait 1500
eval easel JSON.stringify([...document.querySelectorAll('.card.live .slot')].map(c => c.textContent))
cmd {"cmd":"easel_live_play","item":2}
wait 8000
eval easel document.querySelector('.card.live .play').textContent
eval live (() => { const r = document.querySelector('p').getBoundingClientRect(); return 'crop ' + (Math.abs(r.left) < 2 && Math.abs(r.top) < 2 && r.width <= innerWidth + 3 && r.height <= innerHeight + 3); })()
shote 03-easel-live
cmd {"cmd":"easel_live_stop","item":2}
wait 500
eval easel document.querySelector('.card.live .play').textContent
cmd {"cmd":"new_folder"}
wait 600
eval chrome (() => { const i = document.querySelector('.fedit'); if (i) i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' })); return 'x'; })()
wait 600
eval chrome (() => { send({ cmd: 'move_easel', id: state.easels[0].id, folder: state.folders[0].id }); return 'dragged'; })()
wait 1000
eval chrome JSON.stringify({ loose: 0, inFolder: document.querySelectorAll('.fb .tab.easel').length })
eval chrome (() => { document.querySelector('.fb .tab.easel').click(); return 'clicked'; })()
wait 1500
eval etab JSON.stringify({ embedded: document.body.classList.contains('embedded'), nav: getComputedStyle(document.querySelector('nav')).display, cards: document.querySelectorAll('.card').length, name: document.getElementById('title').value })
eval chrome JSON.stringify({ home: state.home, open: state.easels.map(e => e.open) })
cmd {"cmd":"easel_live_play","item":2}
wait 8000
eval live (() => { const r = document.querySelector('p').getBoundingClientRect(); return 'tabcrop ' + (Math.abs(r.left) < 2 && Math.abs(r.top) < 2 && r.width <= innerWidth + 3 && r.height <= innerHeight + 3 && innerWidth > 50); })()
shot 04-easel-tab
cmd {"cmd":"close_easel_tab"}
wait 800
eval chrome JSON.stringify({ home: state.home, open: state.easels.map(e => e.open) })
eval chrome JSON.stringify({ before: document.getElementById('folders').previousElementSibling.className, bms: !!document.getElementById('bms'), bmhead: !!document.getElementById('bmhead') })
cmd {"cmd":"import_html","text":"<DL><DT><H3>ImpFolder</H3><DL><DT><A HREF=\"https://imp1.test/\">Imp One</A><DT><A HREF=\"https://imp2.test/\">Imp Two</A></DL></DL>"}
wait 800
eval chrome (() => { const f = state.folders.find(f => f.name === 'ImpFolder'); const t = f ? state.tabs.filter(t => t.folder === f.id).map(t => t.title) : []; return 'imp ' + JSON.stringify(t) + ' bms ' + document.querySelectorAll('#folders .fh').length; })()
cmd {"cmd":"clear_tabs"}
wait 600
cmd {"cmd":"open_library"}
wait 1000
eval library JSON.stringify({ rail: document.querySelectorAll('.nav').length, archived: S.archive.length > 0, sp: S.spaces.length })
shot 05-library
cmd {"cmd":"close_library"}
wait 500
eval chrome JSON.stringify({ home: state.home })
eval chrome (() => { const t = state.tabs[0]; if (!t) return 'notab'; window.bh.render({ ...state, activeTab: null, media: [{ id: t.id, playing: true, muted: false }] }); const card = document.querySelector('#media .mc'); const btns = card ? [...card.querySelectorAll('button')].map(b => b.textContent) : []; const snd = !!document.querySelector('.tab .snd'); window.bh.render({ ...state, activeTab: t.id, media: [{ id: t.id, playing: true, muted: false }] }); return 'media ' + JSON.stringify({ card: !!card, btns, snd, hiddenWhenActive: !document.querySelector('#media .mc') }); })()
eval chrome (() => { window.bh.render(state); const sp = state.spaces[0]; const switchable = state.spaces.length > 1; editSpace(sp.id); const input = document.querySelector('.space input'); return 'rename ' + JSON.stringify({ input: !!input, value: input ? input.value === sp.name : false }); })()
eval chrome (() => { renderUpdate({ state: 'ready', version: '9.9.9' }, '0.7.4'); const ready = !!document.querySelector('#update .ico svg'); renderUpdate({ state: 'downloading', version: '9.9.9' }, '0.7.4'); const busy = document.querySelector('#update .ico').dataset.k; renderUpdate(null, '0.7.4'); return 'upd ' + JSON.stringify({ ready, busy, text: document.querySelector('#update .ico').textContent }); })()
eval chrome (() => { const i = document.querySelector('.sedit'); if (i) i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })); return 'esc'; })()
wait 500
cmd {"cmd":"import_html","text":"<DL><DT><H3>Outer</H3><DL><DT><H3>Inner</H3><DL><DT><A HREF=\"https://deep.test/\">Deep</A></DL></DL></DL>"}
wait 800
eval chrome (() => { const o = state.folders.find(f => f.name === 'Outer'); const i = state.folders.find(f => f.name === 'Inner'); return 'nest ' + JSON.stringify({ parent: !!o && !!i && i.parent === o.id, top: !!o && o.parent == null, dom: !!document.querySelector('.folder > .fb > .folder') }); })()
eval chrome (() => { const o = state.folders.find(f => f.name === 'Outer'); send({ cmd: 'new_folder', parent: o.id }); return 'sub'; })()
wait 500
eval chrome (() => { const i = document.querySelector('.fedit'); if (i) i.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' })); return 'x'; })()
wait 500
eval chrome (() => { const o = state.folders.find(f => f.name === 'Outer'); return 'kids ' + state.folders.filter(f => f.parent === o.id).length; })()
eval chrome (() => { const o = state.folders.find(f => f.name === 'Outer'); send({ cmd: 'delete_folder', id: o.id }); return 'del'; })()
wait 600
eval chrome JSON.stringify({ folders: state.folders.filter(f => f.name === 'Outer' || f.name === 'Inner' || f.parent != null).length, deep: state.tabs.filter(t => t.title === 'Deep').length })
cmd {"cmd":"open","input":"https://example.com"}
wait 2500
eval content (document.cookie = 'bhmain=one; max-age=600; path=/', document.cookie)
cmd {"cmd":"new_space","name":"Work","color":"#ff8800"}
wait 800
eval chrome (() => { send({ cmd: 'space', id: state.spaces[state.spaces.length - 1].id }); return 'ws' + state.spaces.length; })()
wait 800
cmd {"cmd":"open","input":"https://example.com"}
wait 2500
eval content JSON.stringify({ iso: document.cookie.indexOf('bhmain') < 0, href: location.href, c: document.cookie })
eval content (document.cookie = 'bhwork=two; max-age=600; path=/', document.cookie)
eval chrome (() => { send({ cmd: 'space', id: state.spaces[0].id }); return 'sw'; })()
wait 800
cmd {"cmd":"open","input":"https://example.com"}
wait 2500
eval content JSON.stringify({ main: document.cookie.indexOf('bhmain=one') >= 0, leak: document.cookie.indexOf('bhwork') >= 0 })
quit
'@
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii

$logFile = Join-Path $tmp 'bh_run.log'
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(240000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
$log = Get-Content $logFile
$evals = @($log | Where-Object { $_ -match '\[dev\] eval ->' })
$log | Where-Object { $_ -match '\[dev\] (bad|unknown|eval failed|screenshot failed)' } | ForEach-Object { Write-Host $_ }

$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
Check ($evals[0] -match 'Ideas') 'easel: created and shown'
Check ($evals[1] -match 'Example Domain') 'capture: clip carries the page title'
Check ($evals[2] -match 'example\.com') 'capture: clip shows its source host'
Check ($evals[3] -match '2') 'note: added next to the clip'
Check ($evals[4] -match 'typed') 'note: text typed'
Check ($evals[5] -match '2') 'capture: home page is refused (still 2 cards)'
Check ($evals[6] -match 'example\.com') 'clip link: opens the captured page in a tab'
Check ($evals[7] -match 'Renamed' -and $evals[7] -match 'Second') 'rename / new easel: both listed'
Check ($evals[8] -match '1') 'delete: only one easel left'
Check ($evals[9] -match '1') 'remove: note deleted, one card left'
Check ($evals[10] -match 'true') 'live: picker armed on the page'
Check ($evals[11] -match 'picked') 'live: area clicked'
Check ($evals[12] -match 'Paused') 'live: card added, paused until played'
Check ($evals[13] -match ([string][char]0x23F8)) 'live: playing'
Check ($evals[14] -match 'crop true') 'live: view shows the picked area, scaled to fill the card'
Check ($evals[15] -match ([string][char]0x25B6)) 'live: paused again'
Check ($evals[17] -match 'dragged') 'sidebar: easel dragged onto a folder'
Check ($evals[18] -match 'inFolder.:1') 'sidebar: easel now lives in the folder'
Check ($evals[19] -match 'clicked') 'tab: easel opened from the sidebar'
Check ($evals[20] -match 'embedded.:true') 'tab: easel page is embedded in the main window'
Check ($evals[20] -match 'nav.:.none') 'tab: the list of easels is hidden'
Check ($evals[20] -match 'name.:.Renamed') 'tab: shows the clicked easel'
Check ($evals[21] -match 'open.:\[true\]') 'tab: sidebar marks the easel open'
Check ($evals[22] -match 'tabcrop true') 'tab: live card plays cropped inside the content area'
Check ($evals[23] -match 'open.:\[false\]') 'tab: closing returns to the page'
Check ($evals[24] -match 'before.:.section secrow') 'pinned: header sits right above the folders'
Check ($evals[24] -match 'bms.:false') 'imports: no separate bookmark list, they are folders and tabs'
Check ($evals[25] -match 'Imp One') 'import: bookmarks become tabs filed in a real folder'
Check ($evals[25] -match 'Imp Two') 'import: every link is a regular tab'
Check ($evals[26] -match 'rail\\":5') 'library: five sections in the rail'
Check ($evals[26] -match 'archived\\":true') 'library: cleared tab is archived'
Check ($evals[27] -match 'home') 'library: closes back to the browser'
Check ($evals[28] -match 'card.:true') 'media: a card appears for a tab playing in the background'
Check ($evals[28] -match ([string][char]0x23F8)) 'media: the card offers pause'
Check ($evals[28] -match ([string][char]0x29C9)) 'media: the card offers Picture in Picture'
Check ($evals[28] -match 'snd.:true') 'media: the playing tab shows a speaker icon'
Check ($evals[28] -match 'hiddenWhenActive.:true') 'media: no card for the tab being viewed'
Check ($evals[29] -match 'input.:true') 'space: right-click rename turns the name into an input'
Check ($evals[29] -match 'value.:true') 'space: the input starts with the current name'
Check ($evals[30] -match 'ready.:true') 'update: the button shows a drawn icon, not a font glyph'
Check ($evals[30] -match 'busy.:.busy') 'update: the icon changes while downloading'
Check ($evals[30] -match 'text.:..') 'update: no text glyph left in the icon'
Check ($evals[32] -match 'parent.:true') 'folders: an imported path A/B makes B a subfolder of A'
Check ($evals[32] -match 'top.:true') 'folders: the outer folder stays at the top level'
Check ($evals[32] -match 'dom.:true') 'folders: the sidebar nests the subfolder inside its parent'
Check ($evals[34] -match 'x') 'folders: new subfolder created and named'
Check ($evals[35] -match 'kids 2') 'folders: a folder can hold several subfolders'
Check ($evals[37] -match 'folders.:0') 'folders: deleting a folder removes its subfolders'
Check ($evals[37] -match 'deep.:0') 'folders: tabs of the subfolders are closed with it'
Check ($evals[38] -match 'bhmain=one') 'accounts: a cookie is set in the first space'
Check ($evals[39] -match 'ws2') 'accounts: the new space is listed'
Check ($evals[40] -match 'iso.:true') "accounts: the second space does not see the first space's cookie"
Check ($evals[43] -match 'main.:true') 'accounts: back in the first space its own cookie is still there'
Check ($evals[43] -match 'leak.:false') "accounts: the second space's cookie did not leak into the first"
Check ((Test-Path (Join-Path $tmp 'bh_shots\01-easel-board.png')) -and (Test-Path (Join-Path $tmp 'bh_shots\03-easel-live.png'))) 'screenshots saved'
$file = Join-Path $tmp 'bh_data\easels.json'
$data = if (Test-Path $file) { @(Get-Content $file -Raw | ConvertFrom-Json) } else { @() }
$ok = $data.Count -eq 1 -and $data[0].items.Count -eq 2
$clip = if ($ok) { $data[0].items[0] } else { $null }
$live = if ($ok) { $data[0].items[1] } else { $null }
Check ($ok -and $data[0].name -eq 'Renamed' -and $clip.kind -eq 'clip' -and $clip.url -like 'https://example.com*' -and $live.kind -eq 'live' -and $live.selector -like '*p:nth-of-type*' -and $live.url -like 'https://example.com*') 'persistence: easels.json holds the renamed easel, its clip and the live area'
if ($fail) { Write-Host 'evals:'; $evals }
exit [int]$fail
