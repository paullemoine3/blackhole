# UI scenarios on Windows, driven by the devtools harness (see crates/app/src/devtools.rs).
# Uses the scratch profile %TEMP%\bh_data (wiped first); screenshots land in %TEMP%\bh_shots.
# Needs network access and a visible, uncovered desktop (the window is captured from the screen).
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue

$script = @'
wait 3000
shot 01-home
eval chrome document.body ? 'ready' : 'empty'
cmd {"cmd":"open","input":"https://example.com"}
wait 5000
shot 02-example-com
eval content document.title
cmd {"cmd":"navigate","input":"https://example.org"}
wait 5000
shot 03-second-page
eval content location.href
cmd {"cmd":"back"}
wait 1500
eval content location.href
cmd {"cmd":"palette","mode":"new"}
wait 1500
shot 04-palette
cmd {"cmd":"palette_close"}
wait 1200
cmd {"cmd":"open_settings","tab":"general"}
wait 2000
shots 05-settings-general
cmd {"cmd":"open_settings","tab":"shortcuts"}
wait 1000
shots 06-settings-shortcuts
cmd {"cmd":"open_settings","tab":"extensions"}
wait 1500
shots 07-settings-extensions
eval settings document.querySelector('#catalog') ? 'catalog' : 'missing'
cmd {"cmd":"open_settings","tab":"import"}
wait 1000
shots 08-settings-import
cmd {"cmd":"open_settings","tab":"privacy"}
wait 1000
shots 09-settings-privacy
cmd {"cmd":"close_settings"}
wait 500
cmd {"cmd":"home"}
wait 1000
shot 10-home-again
cmd {"cmd":"set_animations","on":true}
cmd {"cmd":"lock"}
wait 80
eval home window.innerWidth
shot 11-sidebar-mid-hide
wait 700
eval home window.innerWidth
shot 12-sidebar-hidden
cmd {"cmd":"peek","on":true}
wait 500
eval home window.innerWidth
shot 12b-sidebar-flyout
cmd {"cmd":"peek","on":false}
wait 500
cmd {"cmd":"lock"}
wait 700
eval home window.innerWidth
cmd {"cmd":"palette","mode":"new"}
wait 100
eval palette document.body.className
wait 1800
eval palette document.body.className
shot 13-palette-open
cmd {"cmd":"palette_close"}
wait 400
eval palette document.body.className
cmd {"cmd":"set_animations","on":false}
cmd {"cmd":"palette","mode":"new"}
wait 400
eval palette document.body.className
cmd {"cmd":"palette_close"}
wait 300
cmd {"cmd":"set_animations","on":true}
wait 700
eval chrome (bh.render({...state, update:{state:'available',version:'9.9.9'}}), document.getElementById('update').hidden + '|' + document.getElementById('updtxt').textContent)
wait 300
shot 14-update-available
eval chrome (document.getElementById('update').classList.add('hov'), 'x')
wait 350
eval chrome getComputedStyle(document.querySelector('#update .ico')).animationName
shot 15-update-hover
eval chrome (bh.render({...state, update:{state:'downloading',version:'9.9.9'}}), document.getElementById('updtxt').textContent)
wait 300
shot 16-update-downloading
eval chrome (bh.render({...state, update:null}), String(document.getElementById('update').hidden))
wait 300
cmd {"cmd":"window_maximize"}
wait 800
eval bar document.body.className
cmd {"cmd":"window_maximize"}
wait 800
eval bar document.body.className
cmd {"cmd":"new_tab"}
wait 800
cmd {"cmd":"palette_close"}
wait 800
eval chrome (send({cmd:'new_folder'}), 'folder')
wait 800
eval chrome (send({cmd:'move_tab',id:state.tabs[state.tabs.length-1].id,before:null,folder:state.folders[state.folders.length-1].id,pinned:false}), state.tabs.length + '|' + state.folders.length)
wait 800
eval chrome (state.tabs.length + '|' + state.tabs.filter(t => t.folder != null).length)
eval chrome (send({cmd:'delete_folder',id:state.folders[state.folders.length-1].id}), 'deleted')
wait 800
eval chrome (state.tabs.length + '|' + state.tabs.filter(t => t.folder != null).length + '|' + state.folders.length)
eval chrome (showCard({el:document.getElementById('new'),edge:'after'}, false), document.getElementById('dropcard').style.height)
eval chrome (hideCard(), 'x')
eval chrome (bh.downloads([{id:1,name:'big-file.iso',received:50,total:100,state:null},{id:2,name:'unknown.bin',received:2048,total:null,state:null}]), document.querySelector('#dls .dl .pc').textContent + '|' + document.querySelectorAll('#dls .dl').length + '|' + document.querySelector('#dls .dl.unk .pc').textContent)
wait 600
shot 18-download-cards
eval chrome (bh.downloads([{id:1,name:'big-file.iso',received:100,total:100,state:'ok'}]), document.querySelector('#dls .dl').className)
eval chrome (bh.downloads([]), String(document.getElementById('dls').children.length))
cmd {"cmd":"new_tab"}
wait 400
eval palette (document.body.classList.contains('intro') + '|' + document.getElementById('q').value)
shot 19-new-tab-palette
cmd {"cmd":"palette_close"}
wait 500
cmd {"cmd":"open","input":"https://example.com"}
wait 3000
cmd {"cmd":"open","input":"https://example.org"}
wait 3000
eval chrome state.tabs.filter(t => !t.pinned && t.folder == null).map(t => new URL(t.url).host).join(',')
cmd {"cmd":"shortcut","key":"recent_tab"}
wait 800
eval chrome new URL(state.tabs.find(t => t.id === state.activeTab).url).host
cmd {"cmd":"shortcut","key":"w"}
wait 800
eval chrome new URL(state.tabs.find(t => t.id === state.activeTab).url).host + '|' + state.tabs.length
cmd {"cmd":"shortcut","key":"shift+t"}
wait 2500
eval chrome state.tabs.length + '|' + new URL(state.tabs.find(t => t.id === state.activeTab).url).host
shot 20-reopened-tab
eval chrome typeof motion.play + '|' + getComputedStyle(document.querySelector('.tab')).transitionDuration
eval content (document.addEventListener('keydown', e => e.preventDefault(), {once:true}), document.dispatchEvent(new KeyboardEvent('keydown', {key:'f', code:'KeyF', ctrlKey:true, bubbles:true, cancelable:true})), 'own')
wait 600
eval content String([...document.documentElement.children].filter(e => e.tagName === 'DIV').length)
eval content (() => { const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, {acceptNode: n => /\S{4}/.test(n.data) && !/^(SCRIPT|STYLE|NOSCRIPT)$/.test(n.parentElement.tagName) ? 1 : 3}); const n = w.nextNode(); const i = n.data.search(/\S{4}/); const r = document.createRange(); r.setStart(n, i); r.setEnd(n, i + 4); getSelection().removeAllRanges(); getSelection().addRange(r); document.dispatchEvent(new KeyboardEvent('keydown', {key:'f', code:'KeyF', ctrlKey:true, bubbles:true, cancelable:true})); return 'sent'; })()
wait 800
eval content [...document.documentElement.children].filter(e => e.tagName === 'DIV').length + '|' + (CSS.highlights.get('bh-find') ? CSS.highlights.get('bh-find').size : 0) + '|' + document.activeElement.tagName
eval chrome (() => { const other = state.tabs.find(t => t.id !== state.activeTab); const m = [{id: other.id, playing: true, muted: false}]; bh.render({...state, media: m}); const a = document.querySelector('#media .mc'); bh.render({...state, media: m}); bh.render({...state, media: [{...m[0], playing: false}]}); const b = document.querySelector('#media .mc'); const r = (a === b) + '|' + b.querySelector('.ctl button').textContent; bh.render({...state, media: []}); return r; })()
eval bar document.getElementById('sidebar').title + '|' + document.getElementById('back').title
eval palette (() => { bh.open({mode: 'menu', color: '#7c5cff', menu: {x: 10, y: 10, items: [{icon: 'trash', label: 'Delete space', danger: true, confirm: 'Click again', run: {cmd: 'noop'}}]}}); const b = document.querySelector('#menu button'); b.click(); const r = b.classList.contains('armed') + '|' + b.textContent + '|' + !!b.querySelector('svg'); bh.hide(); return r; })()
eval chrome (() => { bh.downloads([{id: 7, name: 'a.iso', received: 10, total: 100, state: null}]); const a = document.querySelector('#dls .dl'); bh.downloads([{id: 7, name: 'a.iso', received: 60, total: 100, state: null}]); const b = document.querySelector('#dls .dl'); const r = (a === b) + '|' + b.querySelector('.pc').textContent + '|' + b.querySelector('.fill').style.width; bh.downloads([]); return r; })()
eval chrome (send({cmd: 'set_space_color', id: state.activeSpace, color: '#2FD4A0'}), 'color')
wait 600
eval chrome state.spaces.find(s => s.id === state.activeSpace).color
cmd {"cmd":"action","id":"key:zoom_in"}
wait 800
eval bar document.getElementById('zoomn').textContent
cmd {"cmd":"action","id":"key:zoom_reset"}
cmd {"cmd":"new_folder"}
wait 700
eval chrome (send({cmd: 'move_tab', id: state.tabs.find(t => t.id !== state.activeTab).id, before: null, folder: state.folders[state.folders.length - 1].id, pinned: false}), 'moved')
wait 700
eval chrome (send({cmd: 'delete_folder', id: state.folders[state.folders.length - 1].id}), 'deleted')
wait 800
eval chrome state.tabs.length + '|' + state.folders.length + '|' + document.getElementById('toast').textContent
eval chrome (document.querySelector('#toast button').click(), 'undo')
wait 800
eval chrome state.tabs.length + '|' + state.folders.length + '|' + state.tabs.filter(t => t.folder != null).length
quit
'@
Set-Content -Path (Join-Path $tmp 'bh_script.txt') -Value $script -Encoding ascii

$logFile = Join-Path $tmp 'bh_run.log'
# Redirecting to a file (rather than capturing the pipe) keeps the app from blocking on its output.
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(240000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
$log = Get-Content $logFile
$evals = @($log | Where-Object { $_ -match '\[dev\] eval ->' })
$saved = @($log | Where-Object { $_ -match '\[dev\] saved' })
$log | Where-Object { $_ -match '\[dev\] (bad|unknown|eval failed|screenshot failed)' } | ForEach-Object { Write-Host $_ }

$fail = $false
function Check($ok, $name) {
    if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail = $true }
}
Check ($evals[0] -match 'ready') 'chrome page loads'
Check ($evals[1] -match 'Example Domain') 'navigation: example.com opens'
Check ($evals[2] -match 'example\.org') 'navigate: example.org loads'
Check ($evals[3] -match 'example\.com') 'back: returns to example.com'
Check ($evals[4] -match 'catalog') 'settings: extension catalog present'
function Width($line) { if ($line -match '(\d+(\.\d+)?)\s*$') { [double]$Matches[1] } else { -1 } }
$mid = Width $evals[5]; $hidden = Width $evals[6]; $peek = Width $evals[7]; $open = Width $evals[8]
Check ($mid -gt $open -and $mid -lt $hidden) "sidebar animation: page width is mid-way while hiding ($mid)"
Check (($hidden - $open) -gt 200 -and ($hidden - $open) -lt 260) "sidebar: hiding gives its width to the page ($hidden vs $open)"
Check ($open -gt 0 -and $open -lt $hidden) "sidebar: toggling again reopens it ($open)"
Check ($peek -eq $hidden) "sidebar flyout: floats over the page without pushing it ($peek)"
Check ($evals[9] -match 'open' -and $evals[9] -match 'pre|pop|intro') 'palette animation: opening plays the intro/pop classes'
Check ($evals[10] -match 'open') 'palette: open after animation'
Check ($evals[11] -notmatch 'open') 'palette: closes'
Check ($evals[12] -match 'open' -and $evals[12] -notmatch 'pre|pop|intro') 'animations off: palette opens without animation classes'
Check ($evals[13] -match 'false\|Update to v9\.9\.9') 'update button: shown with the new version'
Check ($evals[15] -match 'up-bounce') 'update button: hover animation runs'
Check ($evals[16] -match 'Updating to v9\.9\.9') 'update button: downloading state'
Check ($evals[17] -match 'true') 'update button: hidden when up to date'
Check ($evals[18] -match 'windows' -and $evals[18] -notmatch 'maxed') 'window: restore clears the maximized state (it starts maximized)'
Check ($evals[19] -match 'windows' -and $evals[19] -match 'maxed') 'window: maximize button state follows the window'
Check ($evals[22] -match '\|1"?$') 'folder: the tab sits inside the new folder'
$before = if ($evals[22] -match '(\d+)\|1"?$') { [int]$Matches[1] } else { -1 }
Check ($evals[24] -match "^.*\b$($before - 1)\|0\|") 'folder: deleting it closes its tabs instead of ungrouping them'
Check ($evals[25] -match '3px') 'drag & drop: insertion marker is a thin line that overlaps no neighbour'
Check ($evals[27] -match '50%\|2\|2 KB') 'downloads: progress card with percentage, plus an unknown-size card'
Check ($evals[28] -match 'dl ok') 'downloads: finished card'
Check ($evals[29] -match '0') 'downloads: cards disappear'
Check ($evals[30] -match 'true\|"?$') 'new tab: opens the search bar (empty) over the start page'
Check ($evals[31] -match 'example\.org,example\.com') 'tabs: a new tab goes to the top of the list, like Arc'
Check ($evals[32] -match 'example\.com') 'Ctrl+Tab: goes back to the tab seen before'
Check ($evals[33] -match 'example\.org\|1') 'Ctrl/Cmd+W: the tab seen before takes over'
Check ($evals[34] -match '2\|example\.com') 'Ctrl/Cmd+Shift+T: reopens the tab closed last'
Check ($evals[35] -match 'function\|0\.18s') 'sidebar motion: FLIP helper loaded, rows have transitions'
Check ($evals[37] -match '"0"') 'find: a page with its own Ctrl+F (Google Docs) keeps it'
Check ($evals[39] -match '"1\|[1-9]\d*\|DIV"') 'find: Ctrl+F opens the bar on the selection, highlights it and keeps the focus'
Check ($evals[40] -match '"true\|▶"') 'media card: a state refresh updates the same card (no flicker)'
Check ($evals[41] -match 'Ctrl\+B\)\|Back \(Ctrl\+\[ or Alt\+') 'tooltips: shortcuts spelled for Windows (no Cmd sign)'
Check ($evals[42] -match '"true\|Click again\|true"') 'menus: a destructive item asks for a second click and has a drawn icon'
Check ($evals[43] -match '"true\|60%\|60%"') 'download card: progress updates the same card (no flicker)'
Check ($evals[45] -match '#2fd4a0') 'space color: changed from its menu (stored lower-case)'
Check ($evals[46] -match '110%') 'command bar: a shortcut command (zoom in) runs like its keys'
Check ($evals[49] -match '"1\|0\|Folder .*deleted.*Undo"') 'delete folder: its tab goes, a toast offers Undo'
Check ($evals[51] -match '"2\|1\|1"') 'undo: the folder comes back with its tab inside'
Check ($saved.Count -eq 20) "screenshots saved ($($saved.Count)/20)"
if ($fail) { Write-Host "evals:"; $evals }
exit [int]$fail
