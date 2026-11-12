# UI scenarios on Windows, driven by the devtools harness (see crates/app/src/devtools.rs).
# Uses the scratch profile %TEMP%\bh_data (wiped first); screenshots land in %TEMP%\bh_shots.
# Needs network access and a visible, uncovered desktop (the window is captured from the screen).
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
$tmp = [IO.Path]::GetTempPath()
Remove-Item -Recurse -Force (Join-Path $tmp 'bh_data'), (Join-Path $tmp 'bh_shots') -ErrorAction SilentlyContinue

$script = @'
wait 3000
cmd {"cmd":"set_language","language":"en"}
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
eval chrome (() => { const other = state.tabs.find(t => t.id !== state.activeTab); const m = [{id: other.id, playing: true, muted: false}]; bh.render({...state, media: m}); const a = document.querySelector('#media .mc'); bh.render({...state, media: m}); bh.render({...state, media: [{...m[0], playing: false}]}); const b = document.querySelector('#media .mc'); const r = (a === b) + '|' + b.querySelector('.ctl button svg').dataset.i; bh.render({...state, media: []}); return r; })()
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
cmd {"cmd":"open","input":"https://example.net"}
wait 3000
eval chrome (() => { const n = [...document.querySelectorAll('#tabs .tab')].find(n => state.tabs.find(t => t.id == n.dataset.tab).url.includes('example.net')); window.__mid = +n.dataset.tab; n.dispatchEvent(new MouseEvent('auxclick', {button: 1, bubbles: true, cancelable: true})); return 'mid'; })()
wait 900
eval chrome String(state.tabs.some(t => t.id === window.__mid))
eval chrome (send({cmd: 'pin', id: state.activeTab}), 'pin')
wait 800
eval chrome (() => { const n = document.querySelector('#pinned .tab'); window.__pin = +n.dataset.tab; n.dispatchEvent(new MouseEvent('auxclick', {button: 1, bubbles: true, cancelable: true})); send({cmd: 'close', id: window.__pin}); return 'closed'; })()
wait 900
eval chrome state.tabs.filter(t => t.pinned).map(t => t.id).includes(window.__pin) + '|' + document.querySelectorAll('#pinned .snd').length
cmd {"cmd":"open_settings","tab":"general"}
wait 2000
eval settings document.body.classList.contains('windows') + '|' + getComputedStyle(document.getElementById('top')).height + '|' + !!document.querySelector('nav button svg')
cmd {"cmd":"panel_window","window":"settings","action":"close"}
wait 500
cmd {"cmd":"open","input":"https://example.com/?bh=links"}
wait 3500
eval chrome (window.__n = state.tabs.length, window.__act = state.activeTab, 'count')
eval content (() => { const a = document.querySelector('a[href]'); a.dispatchEvent(new MouseEvent('auxclick', {button: 1, bubbles: true, cancelable: true, composed: true})); return 'middle'; })()
wait 2000
eval chrome (state.tabs.length - window.__n) + '|' + (state.activeTab === window.__act)
eval content (() => { const a = document.querySelector('a[href]'); a.dispatchEvent(new MouseEvent('contextmenu', {button: 2, bubbles: true, cancelable: true, composed: true, clientX: 40, clientY: 40})); return 'right'; })()
wait 900
eval palette document.body.classList.contains('menuing') + '|' + [...document.querySelectorAll('#menu button')].map(b => b.textContent).join(',')
cmd {"cmd":"palette_close"}
wait 400
eval content String(window.__bhShields) + '|' + document.adoptedStyleSheets.length
cmd {"cmd":"open_settings","tab":"privacy"}
wait 2000
eval settings [...document.querySelectorAll('#shields .opt')].map(o => o.classList.contains('sel') ? 1 : 0).join('')
cmd {"cmd":"set_shield","name":"video_ads","on":false}
wait 600
eval settings [...document.querySelectorAll('#shields .opt')].map(o => o.classList.contains('sel') ? 1 : 0).join('')
cmd {"cmd":"set_shield","name":"video_ads","on":true}
cmd {"cmd":"panel_window","window":"settings","action":"close"}
wait 500
cmd {"cmd":"capture_live","selector":"body","w":300,"h":200}
wait 900
cmd {"cmd":"capture_live","selector":"body","w":300,"h":200}
wait 900
eval palette [...document.querySelectorAll('#menu .mhead, #menu button')].map(b => b.textContent).join(',')
eval palette (document.querySelector('#menu button').click(), 'picked')
wait 900
eval chrome (state.easels || []).map(e => e.count).join(',')
cmd {"cmd":"open_settings","tab":"calendar"}
wait 2000
eval settings document.querySelector('nav button.on').dataset.tab + '|' + document.getElementById('calsrc').textContent
cmd {"cmd":"calendar_add","url":"https://evil.test/calendar/ical/x/private-y/basic.ics"}
wait 700
eval settings document.getElementById('calstatus').textContent
cmd {"cmd":"calendar_add","url":"https://calendar.google.com/calendar/ical/bh-test%40example.com/private-00000000/basic.ics"}
wait 8000
eval settings document.getElementById('calsrc').textContent + '|' + document.getElementById('calstatus').textContent
cmd {"cmd":"calendar_remove","index":0}
wait 800
eval settings document.getElementById('calsrc').textContent
cmd {"cmd":"panel_window","window":"settings","action":"close"}
wait 500
eval chrome (bh.render({...state, meeting: {title: 'Standup', url: 'https://meet.google.com/abc-defg-hij', when: 'In 5 min · 09:30', on: false}}), document.querySelector('#meet .nm').textContent + '|' + document.querySelector('#meet button:not(.ic)').textContent)
eval chrome (bh.render({...state, meeting: null}), String(document.getElementById('meet').children.length))
eval chrome (window.__n = state.tabs.length, 'count')
cmd {"cmd":"open_meet"}
wait 3500
eval chrome (state.tabs.length - window.__n) + '|' + state.tabs.some(t => /google\.com/.test(t.url))
cmd {"cmd":"open","input":"https://example.com/?bh=report"}
wait 2500
cmd {"cmd":"privacy_report","x":20,"y":500}
wait 900
eval palette [...document.querySelectorAll('#menu .mhead, #menu button')].map(b => b.textContent).join(',')
cmd {"cmd":"palette_close"}
wait 400
cmd {"cmd":"open","input":"https://example.com/?tidy=1"}
wait 700
cmd {"cmd":"open","input":"https://example.com/?tidy=2"}
wait 700
cmd {"cmd":"open","input":"https://example.com/?tidy=3"}
wait 700
cmd {"cmd":"open","input":"https://example.com/?tidy=4"}
wait 1200
eval chrome JSON.stringify(state.tidy && state.tidy.groups.map(g => g.name)) + '|' + !!document.querySelector('#tidy .go')
eval chrome (window.__f = state.folders.length, document.querySelector('#tidy .go').click(), 'tidy')
wait 1000
eval chrome (state.folders.length - window.__f) + '|' + (state.tidy === null) + '|' + document.getElementById('toast').textContent
eval chrome (document.querySelector('#toast button').click(), 'undo')
wait 1000
eval chrome String(state.folders.length - window.__f)
cmd {"cmd":"action","id":"compare"}
wait 2500
eval palette document.body.classList.contains('comparing') + '|' + (document.querySelectorAll('#cpick label').length >= 2)
cmd {"cmd":"palette_close"}
wait 400
eval palette (() => { bh.open({mode: 'compare', color: '#7c5cff', compare: {items: [{id: 1, title: 'A', url: 'https://a.test/', name: 'Phone A', price: '199', currency: 'EUR', rating: '4.5', specs: {Weight: '200 g'}}, {id: 2, title: 'B', url: 'https://b.test/', name: 'Phone B', price: '179', currency: 'EUR', rating: '4.1', specs: {Weight: '250 g'}}]}}); const r = [...document.querySelectorAll('#ctable tr')].map(tr => tr.textContent).join('/') + '|' + [...document.querySelectorAll('#ctable .best')].map(td => td.textContent).join(','); bh.hide(); return r; })()
cmd {"cmd":"open","input":"https://example.com/?bh=select"}
wait 3500
eval content (() => { window.__sel = (el) => { const r = document.createRange(); r.selectNodeContents(el); const s = getSelection(); s.removeAllRanges(); s.addRange(r); const b = r.getBoundingClientRect(); el.dispatchEvent(new MouseEvent('contextmenu', {button: 2, bubbles: true, cancelable: true, composed: true, clientX: b.left + 4, clientY: b.top + b.height / 2})); }; const p = document.createElement('p'); p.id = 'bhsel'; p.textContent = 'Black hole sun'; const a = document.createElement('a'); a.id = 'bhlink'; a.href = 'https://example.net/'; a.textContent = 'Black hole link'; document.body.append(p, a); window.__sel(p); return 'selected'; })()
wait 900
shot 21-selection-menu
eval palette document.body.classList.contains('menuing') + '|' + [...document.querySelectorAll('#menu button')].map(b => b.textContent).join(',')
eval chrome (window.__n = state.tabs.length, window.__act = state.activeTab, 'count')
eval palette (document.querySelector('#menu button').click(), 'search')
wait 2500
eval chrome (state.tabs.length - window.__n) + '|' + (state.activeTab !== window.__act) + '|' + (state.tabs.find(t => t.id === state.activeTab) || {}).url
cmd {"cmd":"open","input":"https://example.com/?bh=address"}
wait 3500
eval content (() => { window.__sel = (el) => { const r = document.createRange(); r.selectNodeContents(el); const s = getSelection(); s.removeAllRanges(); s.addRange(r); const b = r.getBoundingClientRect(); el.dispatchEvent(new MouseEvent('contextmenu', {button: 2, bubbles: true, cancelable: true, composed: true, clientX: b.left + 4, clientY: b.top + b.height / 2})); }; const p = document.createElement('p'); p.id = 'bhsel'; p.textContent = 'example.org/bh-sel'; const a = document.createElement('a'); a.id = 'bhlink'; a.href = 'https://example.net/'; a.textContent = 'Black hole link'; document.body.append(p, a); window.__sel(p); return 'address'; })()
wait 900
eval palette [...document.querySelectorAll('#menu button')].map(b => b.textContent).join(',')
eval chrome (window.__n = state.tabs.length, window.__act = state.activeTab, 'count')
eval palette (document.querySelector('#menu button').dispatchEvent(new MouseEvent('click', {ctrlKey: true, bubbles: true, cancelable: true})), 'background')
wait 2000
eval chrome (state.tabs.length - window.__n) + '|' + (state.activeTab === window.__act) + '|' + state.tabs.some(t => t.url === 'https://example.org/bh-sel')
eval content (document.getElementById('bhsel').textContent = 'javascript:alert(document.cookie)', window.__sel(document.getElementById('bhsel')), 'js')
wait 900
eval palette [...document.querySelectorAll('#menu button')].map(b => b.textContent).join(',')
cmd {"cmd":"palette_close"}
wait 400
eval content (window.__sel(document.getElementById('bhlink')), 'link')
wait 900
eval palette [...document.querySelectorAll('#menu button')].map(b => b.textContent).join(',')
cmd {"cmd":"palette_close"}
wait 400
cmd {"cmd":"open_settings","tab":"devices"}
wait 1500
shots 22-settings-devices
eval settings document.querySelector('section.on').id + '|' + document.getElementById('devlist').textContent
cmd {"cmd":"device_join","code":"nope"}
wait 800
eval settings document.getElementById('devstatus').textContent
cmd {"cmd":"device_show_code"}
wait 2500
eval settings document.getElementById('devcode').hidden + '|' + document.getElementById('devcodetext').textContent
cmd {"cmd":"device_cancel_pairing"}
wait 800
eval settings String(document.getElementById('devcode').hidden)
cmd {"cmd":"close_settings"}
wait 400
cmd {"cmd":"open","input":"http://127.0.0.1:__AUTH_PORT__/"}
wait 3000
eval palette 'auth1:' + document.body.classList.contains('authing') + '|' + document.getElementById('ahost').textContent + '|' + document.getElementById('arealm').textContent + '|' + document.getElementById('awarn').textContent
shot 23-http-auth
eval palette 'auth2:' + (document.getElementById('auser').value = '__AUTH_USER__', document.getElementById('apass').value = 'wrong-password', document.getElementById('aok').click(), 'sent')
wait 2000
eval palette 'auth3:' + document.body.classList.contains('authing') + '|' + document.getElementById('auser').value + '|' + document.getElementById('apass').value.length + '|' + document.getElementById('awarn').textContent
eval palette 'auth4:' + (document.getElementById('apass').value = '__AUTH_PASSWORD__', document.getElementById('aok').click(), 'sent')
wait 2500
eval content 'auth5:' + document.title + '|' + (document.querySelector('h1') || {}).textContent
eval palette 'auth6:' + document.body.classList.contains('authing') + '|' + document.getElementById('apass').value.length
cmd {"cmd":"open","input":"http://127.0.0.1:__AUTH_PORT__/forbidden"}
wait 2500
eval content 'auth7:' + document.title
cmd {"cmd":"open","input":"http://127.0.0.1:__AUTH_PORT__/other"}
wait 2500
eval palette 'auth8:' + document.body.classList.contains('authing') + '|' + document.getElementById('arealm').textContent + '|' + (document.getElementById('acancel').click(), 'cancelled')
wait 2000
eval content 'auth9:' + document.title
eval palette 'auth10:' + document.body.classList.contains('authing')
cmd {"cmd":"open","input":"http://127.0.0.1:__AUTH_PORT__/other?esc"}
wait 2500
cmd {"cmd":"palette_close"}
wait 1500
eval content 'auth11:' + document.title
cmd {"cmd":"open_settings","tab":"passwords"}
wait 1500
eval settings 'pw1:' + document.querySelector('section.on').id + '|' + document.getElementById('pwopen').hidden + '|' + document.getElementById('pwlist').textContent
cmd {"cmd":"passwords","action":{"op":"save","origin":"example.com","username":"bh-test-user","password":"__PW_TEST__"}}
wait 800
eval settings 'pw2:' + document.getElementById('pwlist').textContent + '|' + document.body.innerHTML.includes('__PW_TEST__')
eval settings 'pw3:' + (document.getElementById('pwadd').click(), document.getElementById('pwgen').click(), 'generate')
wait 600
eval settings 'pw4:' + document.getElementById('pwpass').value.length + '|' + !document.getElementById('pwedit').hidden
shots 24-settings-passwords
eval settings 'pw5:' + (document.getElementById('pwcancel').click(), 'closed')
cmd {"cmd":"close_settings"}
wait 400
cmd {"cmd":"open","input":"https://example.com/"}
wait 3500
eval content 'pw6:' + (document.body.insertAdjacentHTML('beforeend', '<form id="bhf" onsubmit="event.preventDefault()"><input id="bhu" autocomplete="username"><input id="bhp" type="password"><button id="bhs">Sign in</button></form>'), document.getElementById('bhp').focus(), typeof window.__bhAutofill)
wait 800
eval content 'pw7:' + (window.__bhipc.postMessage(JSON.stringify({cmd: 'autofill_fill', index: 0})), window.__bhipc.postMessage(JSON.stringify({cmd: 'autofill_fill', index: 0, token: 'guess'})), 'forged')
wait 800
eval content 'pw8:' + document.getElementById('bhp').value.length + '|' + document.getElementById('bhu').value
eval content 'pw9:' + (window.__bhipc.postMessage(JSON.stringify({cmd: 'login_submitted', username: 'bh-test-user', password: '__PW_TEST2__'})), 'submitted')
wait 800
eval chrome 'pw10:' + document.getElementById('toast').textContent
eval chrome 'pw11:' + (document.querySelector('#toast button').click(), 'update')
wait 600
cmd {"cmd":"open_settings","tab":"passwords"}
wait 1500
eval settings 'pw12:' + ([...document.querySelectorAll('#pwlist button')].find(b => b.textContent === 'Show').click(), 'show')
wait 600
eval settings 'pw13:' + document.querySelectorAll('#pwlist .row').length + '|' + (document.querySelector('#pwlist code') || {}).textContent
eval settings 'pw14:' + (() => { const d = [...document.querySelectorAll('#pwlist button')].find(b => b.textContent === 'Delete'); d.click(); d.click(); return 'deleted'; })()
wait 800
eval settings 'pw15:' + document.getElementById('pwlist').textContent
eval chrome 'pw16:' + document.getElementById('toast').textContent
cmd {"cmd":"close_settings"}
wait 400
cmd {"cmd":"open","input":"http://127.0.0.1:__ERR_PORT__/e401"}
wait 2500
eval content 'err1:' + document.title + '|' + document.getElementById('actions').textContent
shot 25-error-401
cmd {"cmd":"open","input":"http://127.0.0.1:__ERR_PORT__/e403"}
wait 2500
eval content 'err2:' + document.title + '|' + document.getElementById('hint').textContent
shot 26-error-403
cmd {"cmd":"open","input":"http://127.0.0.1:__ERR_PORT__/e502"}
wait 2500
eval content 'err3:' + document.title + '|' + document.getElementById('hint').textContent
shot 27-error-5xx
cmd {"cmd":"open","input":"http://127.0.0.1:__ERR_PORT__/loop"}
wait 4000
eval content 'err4:' + document.title
shot 28-error-redirects
cmd {"cmd":"open","input":"http://bh-missing.invalid/"}
wait 4000
eval content 'err5:' + document.title + '|' + document.getElementById('hint').textContent
shot 29-error-dns
cmd {"cmd":"open","input":"https://127.0.0.1:__TLS_PORT__/"}
wait 3500
eval content 'err6:' + document.title + '|' + document.getElementById('facts').textContent + '|' + document.getElementById('risky').textContent
eval content 'err7:' + (document.getElementById('advanced').open = true, 'open')
wait 400
shot 30-error-certificate
eval content 'err8:' + (window.__bhipc.postMessage(JSON.stringify({cmd: 'error_proceed', tab: 1})), document.querySelector('#risky button').click(), 'asked')
wait 900
eval palette 'err9:' + document.body.className + '|' + document.getElementById('menu').textContent
shot 31-error-certificate-menu
eval palette 'err10:' + ([...document.querySelectorAll('#menu button')].find(b => /not secure/.test(b.textContent)).click(), 'proceed')
wait 3500
eval content 'err11:' + document.title
eval bar 'err12:' + document.getElementById('insecure').hidden + '|' + document.body.classList.contains('insecure') + '|' + document.body.classList.contains('secure') + '|' + getComputedStyle(document.body).position + '|' + (document.getElementById('omni').getBoundingClientRect().width > 300)
shot 32-insecure-bar
cmd {"cmd":"open","input":"https://example.com/"}
wait 3500
eval content 'err13:' + (window.__bhipc.postMessage(JSON.stringify({cmd: 'error_menu', x: 10, y: 10})), 'forged')
wait 900
eval palette 'err14:' + document.body.className
eval bar 'err15:' + document.getElementById('insecure').hidden
eval content 'err16:' + (location.href = 'https://stats.g.doubleclick.net/bh-test', 'go')
wait 2000
eval content 'err17:' + document.title + '|' + document.getElementById('actions').textContent
shot 33-error-blocked
errpage offline
wait 800
eval content 'err18:' + document.title
shot 34-error-offline
errpage clock
wait 800
eval content 'err19:' + document.title + '|' + document.getElementById('risky').textContent
shot 35-error-clock
errpage http_only
wait 800
eval content 'err20:' + document.title + '|' + document.getElementById('risky').textContent
shot 36-error-http-only
errpage unreachable
wait 800
eval content 'err21:' + document.title + '|' + !document.querySelector('.hole').hidden + '|' + document.querySelectorAll('svg:not([hidden])').length
shot 37-error-unreachable
# ---- a link that downloads a file, the hidden sidebar, the page's menu ----
cmd {"cmd":"open","input":"http://127.0.0.1:__ERR_PORT__/dlpage"}
wait 3500
cmd {"cmd":"lock"}
wait 900
eval content 'dl1:' + (document.getElementById('dl').click(), document.title)
wait 1500
shot 38-download-sidebar-revealed
eval content 'dl2:' + document.title + '|' + location.pathname
wait 6500
eval content 'dl3:' + document.title + '|' + location.pathname
shot 39-download-sidebar-back
eval content 'cm1:' + (document.getElementById('txt').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 300, clientY: 200 })), 'sent')
wait 1200
eval palette 'cm2:' + [...document.querySelectorAll('#menu button')].map(b => b.textContent).join('/')
shot 40-page-menu
cmd {"cmd":"palette_close"}
wait 600
eval content 'cm3:' + (document.getElementById('im').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 60, clientY: 150 })), 'sent')
wait 1200
eval palette 'cm4:' + [...document.querySelectorAll('#menu button')].slice(0, 2).map(b => b.textContent).join('/')
cmd {"cmd":"palette_close"}
wait 600
eval content 'cm7:' + (document.getElementById('pv').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 40, clientY: 300 })), 'preview')
wait 1200
eval palette 'cm8:' + document.body.className + '|' + [...document.querySelectorAll('#menu button')].map(b => b.textContent).join('/').includes('Capture a live area')
cmd {"cmd":"palette_close"}
wait 600
eval content 'cm9:' + (document.getElementById('cv').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 130, clientY: 300 })), 'controls')
wait 1000
eval palette 'cm10:' + document.body.className
cmd {"cmd":"palette_close"}
wait 600
eval content 'cm5:' + (document.addEventListener('contextmenu', e => e.preventDefault()), document.getElementById('txt').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 300, clientY: 200 })), 'own')
wait 1000
eval palette 'cm6:' + document.body.className
# ---- the sidebar's rows stay when only a counter moves ----
eval chrome 'sb1:' + (document.querySelector('#tabs .tab').__kept = 1, bh.render(Object.assign({}, state, { blocked: state.blocked + 3 })), (document.querySelector('#tabs .tab').__kept === 1) + '|' + document.getElementById('blockedn').textContent.trim())
eval chrome 'sb2:' + (bh.render(Object.assign({}, state, { tabs: state.tabs.map(t => Object.assign({}, t, { title: t.title + '!' })) })), document.querySelector('#tabs .tab').__kept === 1)
eval chrome 'sb3:' + (bh.render(Object.assign({}, state, { folders: [{ id: 9001, name: 'Closed', icon: '', collapsed: true, parent: null }, { id: 9002, name: 'Inner', icon: '', collapsed: false, parent: 9001 }, { id: 9003, name: 'Idle', icon: '', collapsed: true, parent: null }], tabs: [[9101, 9001, false], [9102, 9001, true], [9103, 9002, false], [9104, 9002, true], [9105, 9003, true]].map(([id, folder, asleep]) => Object.assign({}, state.tabs[0], { id, folder, asleep, pinned: false })) })), [9101, 9102, 9103, 9104, 9105].map(id => getComputedStyle(document.querySelector('.tab[data-tab="' + id + '"]')).display !== 'none' && getComputedStyle(document.querySelector('.tab[data-tab="' + id + '"]').parentNode).display !== 'none').join(',') + '|' + [...document.querySelectorAll('.fh.idle .fname')].map(n => n.textContent).join(','))
quit
'@
# HTTP authentication: a local server answers 401 Basic (test-only credentials from the fixture).
$authFixture = Get-Content (Join-Path $PSScriptRoot 'fixtures\http-auth.json') -Raw | ConvertFrom-Json
$authPort = 18417
# Password wallet: generated test passwords only (never real ones).
$pwTest = 'bh-test-' + [guid]::NewGuid().ToString('N').Substring(0, 16)
$pwTest2 = 'bh-test-' + [guid]::NewGuid().ToString('N').Substring(0, 16)
$script = $script.Replace('__PW_TEST2__', $pwTest2).Replace('__PW_TEST__', $pwTest)
$script = $script.Replace('__AUTH_PORT__', "$authPort").Replace('__AUTH_USER__', $authFixture.user).Replace('__AUTH_PASSWORD__', $authFixture.password)
# Error pages: local http server (empty 401/403/502 answers, a redirect loop) and a local https
# server with a self-signed certificate made for this run (in memory, nothing installed).
$errPort = 18418
$tlsPort = 18419
$script = $script.Replace('__ERR_PORT__', "$errPort").Replace('__TLS_PORT__', "$tlsPort")
$errServer = Start-Job -ArgumentList $errPort, $tlsPort -ScriptBlock {
    param($port, $tlsPort)
    $plain = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $port)
    $secure = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $tlsPort)
    $plain.Start(); $secure.Start()
    $X = [Security.Cryptography.X509Certificates.X509Certificate2]
    $rsa = [Security.Cryptography.RSA]::Create(2048)
    $req = [Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=127.0.0.1', $rsa,
        [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
    $san = [Security.Cryptography.X509Certificates.SubjectAlternativeNameBuilder]::new()
    $san.AddIpAddress([Net.IPAddress]::Loopback)
    $req.CertificateExtensions.Add($san.Build())
    $made = $req.CreateSelfSigned([DateTimeOffset]::Now.AddDays(-1), [DateTimeOffset]::Now.AddDays(7))
    # SChannel needs the key in a key container: round trip through PKCS#12.
    $cert = $X::new($made.Export('Pfx', 'bh'), 'bh', 'UserKeySet')
    $answer = {
        param($stream)
        $reader = [IO.StreamReader]::new($stream, [Text.Encoding]::ASCII, $false, 4096, $true)
        $path = (($reader.ReadLine()) -split ' ')[1]
        while ($reader.ReadLine()) {}
        $extra = ''; $body = ''; $type = 'text/html; charset=utf-8'
        if ($path -like '/e401*') { $status = '401 Unauthorized' }
        elseif ($path -like '/e403*') { $status = '403 Forbidden' }
        elseif ($path -like '/e502*') { $status = '502 Bad Gateway' }
        elseif ($path -like '/loop*') { $status = '302 Found'; $extra = "Location: /loop`r`n" }
        elseif ($path -like '/favicon*') { $status = '404 Not Found' }
        elseif ($path -like '/dlpage*') { $status = '200 OK'; $body = '<title>bh-dl-page</title><h1>Download test</h1><p><a id=dl href=/dlfile>Download the file</a></p><img id=im src=/favicon.ico width=120 height=80><p id=txt>Plain text of the page.</p><video id=pv width=80 height=40></video><video id=cv controls width=80 height=40></video>' }
        elseif ($path -like '/dlfile*') { $status = '200 OK'; $type = 'application/zip'; $extra = "Content-Disposition: attachment; filename=`"bh-test-download.zip`"`r`n"; $body = 'x' * 4000 }
        else { $status = '200 OK'; $body = '<title>bh-tls-ok</title><h1>Self-signed test page</h1>' }
        $bytes = [Text.Encoding]::UTF8.GetBytes($body)
        $head = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 $status`r`nContent-Type: $type`r`nContent-Length: $($bytes.Length)`r`nConnection: close`r`n$extra`r`n")
        $stream.Write($head, 0, $head.Length)
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush()
    }
    $deadline = (Get-Date).AddMinutes(8)
    while ((Get-Date) -lt $deadline) {
        if ($plain.Pending()) {
            $client = $plain.AcceptTcpClient()
            try { & $answer $client.GetStream() } catch {} finally { $client.Close() }
        } elseif ($secure.Pending()) {
            $client = $secure.AcceptTcpClient()
            try {
                $client.ReceiveTimeout = 3000
                $ssl = [Net.Security.SslStream]::new($client.GetStream(), $false)
                $ssl.AuthenticateAsServer($cert, $false, [Security.Authentication.SslProtocols]::Tls12, $false)
                & $answer $ssl
            } catch {} finally { $client.Close() }
        } else { Start-Sleep -Milliseconds 30 }
    }
    $plain.Stop(); $secure.Stop()
}
$authServer = Start-Job -ArgumentList $authPort, $authFixture.user, $authFixture.password -ScriptBlock {
    param($port, $user, $password)
    $expected = 'Basic ' + [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("${user}:${password}"))
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $port)
    $listener.Start()
    $deadline = (Get-Date).AddMinutes(8)
    while ((Get-Date) -lt $deadline) {
        if (-not $listener.Pending()) { Start-Sleep -Milliseconds 30; continue }
        $client = $listener.AcceptTcpClient()
        try {
            $stream = $client.GetStream()
            $reader = [IO.StreamReader]::new($stream, [Text.Encoding]::ASCII, $false, 4096, $true)
            $path = (($reader.ReadLine()) -split ' ')[1]
            $auth = ''
            while ($line = $reader.ReadLine()) { if ($line -match '^Authorization:\s*(.+)$') { $auth = $Matches[1].Trim() } }
            $extra = ''
            if ($path -like '/favicon*') { $status = '404 Not Found'; $body = 'none' }
            elseif ($path -like '/forbidden*') { $status = '403 Forbidden'; $body = '<title>bh-403</title><h1>403 Forbidden</h1>' }
            elseif ($path -notlike '/other*' -and $auth -eq $expected) { $status = '200 OK'; $body = '<title>bh-auth-ok</title><h1>Signed in</h1>' }
            else {
                $realm = if ($path -like '/other*') { 'Other test' } else { 'BlackHole test' }
                $status = '401 Unauthorized'
                $extra = "WWW-Authenticate: Basic realm=`"$realm`"`r`n"
                $body = '<title>bh-auth-401</title><h1>401 Unauthorized</h1>'
            }
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
# Redirecting to a file (rather than capturing the pipe) keeps the app from blocking on its output.
$proc = Start-Process cmd -ArgumentList '/c', "cargo run -q -p blackhole --features devtools > `"$logFile`" 2>&1" -PassThru -WindowStyle Hidden
if (-not $proc.WaitForExit(420000)) { Stop-Process -Id $proc.Id; Write-Host 'FAIL timeout' }
$log = Get-Content $logFile
# The HTTP authentication evals are tagged ("authN:") and read apart: they shift no index.
$all = @($log | Where-Object { $_ -match '\[dev\] eval ->' })
$evals = @($all | Where-Object { $_ -notmatch '-> "(auth|pw|err|dl|cm|sb)\d+:' })
function AuthEval($n) { $all | Where-Object { $_ -match "-> `"auth${n}:" } | Select-Object -First 1 }
# The password wallet evals too ("pwN:").
function PwEval($n) { $all | Where-Object { $_ -match "-> `"pw${n}:" } | Select-Object -First 1 }
# The error page evals too ("errN:").
function ErrEval($n) { $all | Where-Object { $_ -match "-> `"err${n}:" } | Select-Object -First 1 }
Stop-Job $authServer -ErrorAction SilentlyContinue; Remove-Job $authServer -Force -ErrorAction SilentlyContinue
Stop-Job $errServer -ErrorAction SilentlyContinue; Remove-Job $errServer -Force -ErrorAction SilentlyContinue
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
Check ($mid -eq $hidden -and $mid -gt $open) "sidebar animation: the page slides at its final width, not resized on every frame ($mid)"
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
Check ($evals[40] -match '"true\|play"') 'media card: a state refresh updates the same card (no flicker)'
Check ($evals[41] -match 'Ctrl\+B\)\|Back \(Ctrl\+\[ or Alt\+') 'tooltips: shortcuts spelled for Windows (no Cmd sign)'
Check ($evals[42] -match '"true\|Click again\|true"') 'menus: a destructive item asks for a second click and has a drawn icon'
Check ($evals[43] -match '"true\|60%\|60%"') 'download card: progress updates the same card (no flicker)'
Check ($evals[45] -match '#2fd4a0') 'space color: changed from its menu (stored lower-case)'
Check ($evals[46] -match '110%') 'command bar: a shortcut command (zoom in) runs like its keys'
Check ($evals[49] -match '"1\|0\|Folder .*deleted.*Undo"') 'delete folder: its tab goes, a toast offers Undo'
Check ($evals[51] -match '"2\|1\|1"') 'undo: the folder comes back with its tab inside'
Check ($evals[53] -match '"false"') 'middle click: closes an ordinary tab'
Check ($evals[56] -match '"true\|0"') 'quick access: closing (middle click, close) keeps the tile, no speaker on tiles'
Check ($evals[57] -match '"true\|38px\|true"') 'settings window: own top bar like the main window, drawn icons'
Check ($evals[60] -match '"1\|true"') 'middle click on a link: a new tab in the background'
Check ($evals[62] -match 'true\|Open link in new tab,Open link in new window,Open link in private window' -and $evals[62] -match 'Copy link,Add this page to the Easel,Capture a live area') 'right click on a link: the link menu, then the Easel and live area entries'
Check ($evals[63] -match '"true\|[1-9]') 'shields: the clean-up script and its sheet are in the page'
Check ($evals[64] -match '"11111"' -and $evals[65] -match '"11011"') 'privacy: shields listed and switched one by one'
Check ($evals[66] -match 'Add the live area to,.*New Easel,Watch this area for changes') 'live capture: asks which Easel, or to watch the area'
Check ($evals[68] -match '"1"') 'live capture: added to the Easel chosen (a new one)'
Check ($evals[69] -match '"calendar\|No calendar yet\."') 'calendar: its own section in Settings'
Check ($evals[70] -match 'secret address') 'calendar: only a Google Calendar secret address is accepted'
Check ($evals[71] -match 'bh-test@example\.com' -and $evals[71] -notmatch 'private-0') 'calendar: added, shown by its name only (the secret stays hidden)'
Check ($evals[71] -match 'not found|HTTP 4') 'calendar: synced over https with calendar.google.com (a fake address is reported)'
Check ($evals[72] -match 'No calendar yet') 'calendar: removed'
Check ($evals[73] -match '"Standup\|Join"') 'meeting card: the next meeting with a Join button'
Check ($evals[74] -match '"0"') 'meeting card: gone when there is no meeting'
Check ($evals[76] -match '"1\|true"') 'New Google Meet: opens Meet in a new tab'
Check ($evals[77] -match 'example\.com · grade [A-E],.*Privacy settings') 'privacy report: the site shown, its grade and what was blocked'
Check ($evals[78] -match 'example\.com' -and $evals[78] -match '\|true"') 'tidy: the sidebar suggests a folder for the busiest site'
Check ($evals[80] -match '"1\|true\|.*filed into 1 folder.*Undo"') 'tidy: tabs filed into a folder, with Undo'
Check ($evals[82] -match '"0"') 'tidy: Undo puts the tabs back'
Check ($evals[83] -match '"true\|true"') 'compare: the open tabs are read and shown side by side'
Check ($evals[84] -match 'Price199 EUR179 EUR' -and $evals[84] -match 'Weight200 g250 g' -and $evals[84] -match '\|179 EUR,4\.5"') 'compare: fields in rows, lowest price and best rating stand out'
Check ($evals[86] -match '"true\|Search for .{1,3}Black hole sun.{1,3},Copy,Highlight,Add this page to the Easel,') 'selection: right click offers to search it, or copy it'
Check ($evals[89] -match '"1\|true\|https://duckduckgo\.com/\?q=Black\+hole\+sun') 'selection: the search opens in a new tab, in front, with the engine of the Settings'
Check ($evals[91] -match 'Open address .{1,3}example\.org/bh-sel') 'selection: an address is offered to be opened instead'
Check ($evals[94] -match '"1\|true\|true"') 'selection: Ctrl+click on the entry opens it in the background'
Check ($evals[96] -match 'Search for .{1,3}javascript:alert' -and $evals[96] -notmatch 'Open address') 'selection: a javascript: address is only ever searched'
Check ($evals[98] -match 'Search for .{1,3}Black hole link.{1,3},Copy,Highlight,Open link in new tab,.*Open link in black hole tab,.*Copy link') 'selection on a link: search entries plus the link menu'
Check ($evals[99] -match '"devices\|No device paired yet\."') 'devices: their own section in Settings, nothing paired'
Check ($evals[100] -match '12 characters') 'devices: a wrong pairing code is refused'
Check ($evals[101] -match '"false\|[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}"') 'devices: a pairing code is shown (ABCD-EFGH-JKMN)'
Check ($evals[102] -match '"true"') 'devices: cancelling hides the code'
Check ((AuthEval 1) -match 'auth1:true\|127\.0\.0\.1\|The site says: .{1,3}BlackHole test.{1,3}\|.*not secure') 'http auth: a 401 Basic page asks for a user name and password (realm, clear-text warning)'
Check ((AuthEval 3) -match 'auth3:true\|bh-test-user\|0\|Wrong user name or password') 'http auth: refused credentials ask again, user name kept, password cleared'
Check ((AuthEval 5) -match 'auth5:bh-auth-ok\|Signed in') 'http auth: the right credentials open the page'
Check ((AuthEval 6) -match 'auth6:false\|0') 'http auth: the prompt closes and keeps no password'
Check ((AuthEval 7) -match 'auth7:bh-403') 'http status: a 403 page is the server page, not "Site unreachable"'
Check ((AuthEval 8) -match 'auth8:true\|The site says: .{1,3}Other test.{1,3}\|cancelled') 'http auth: another realm asks again'
Check ((AuthEval 9) -match 'auth9:bh-auth-401') 'http auth: Cancel shows the server 401 page, not "Site unreachable"'
Check ((AuthEval 10) -match 'auth10:false') 'http auth: Cancel closes the prompt'
Check ((AuthEval 11) -match 'auth11:bh-auth-401') 'http auth: closing the palette (Esc) cancels like Cancel'
Check ((PwEval 1) -match 'pw1:passwords\|false\|No saved password yet') 'passwords: their own section in Settings, open with the keychain, empty'
Check ((PwEval 2) -match 'pw2:E?example\.combh-test-user.*\|false') 'passwords: added, listed by site and username, the password not in the page'
Check ((PwEval 4) -match 'pw4:20\|true') 'passwords: the generator fills a 20-character password in the editor'
Check ((PwEval 6) -match 'pw6:object') 'passwords: the autofill script is in the page (main frame)'
Check ((PwEval 8) -match 'pw8:0\|"') 'passwords: a page posting autofill_fill itself (no menu token) gets nothing'
Check ((PwEval 10) -match 'pw10:Update the password for bh-test-user on example\.com\?UpdateNever') 'passwords: a submitted login with a new password offers Update / Never'
Check ((PwEval 13) -match "pw13:1\|$pwTest2") 'passwords: Update kept one entry with the new password, shown on request'
Check ((PwEval 15) -match 'pw15:No saved password yet') 'passwords: deleted after a second click'
Check ((PwEval 16) -match 'pw16:Password for example\.com deleted.*Undo') 'passwords: deleting offers Undo'
Check ((ErrEval 1) -match 'err1:Sign-in required\|Sign in') 'error page: an empty 401 shows "Sign-in required" with "Sign in"'
Check ((ErrEval 2) -match 'err2:Access denied\|.*127\.0\.0\.1') 'error page: an empty 403 shows the forbidden page'
Check ((ErrEval 3) -match 'err3:The server has a problem \(502\)\|.*502') 'error page: an empty 502 shows the server error page'
Check ((ErrEval 4) -match 'err4:Too many redirects') 'error page: a redirect loop shows "Too many redirects"'
Check ((ErrEval 5) -match 'err5:Address not found\|.*bh-missing\.invalid') 'error page: an unknown name shows "Address not found"'
Check ((ErrEval 6) -match 'err6:Connection not secure\|Host127\.0\.0\.1Reason.*Continue anyway') 'error page: a self-signed certificate shows the details and the risky button'
Check ((ErrEval 9) -match 'err9:.*open.*\|.*Continue to 127\.0\.0\.1 \(not secure\)') 'error page: the risky button opens the shell menu (a forged error_proceed from the page did nothing)'
Check ((ErrEval 11) -match 'err11:bh-tls-ok') 'error page: after the menu, the self-signed site opens'
Check ((ErrEval 12) -match 'err12:false\|true\|false\|static\|true') 'error page: the bar says "Not secure" after the exception, its layout unchanged'
Check ((ErrEval 14) -match 'err14:' -and (ErrEval 14) -notmatch 'open')'error page: a web page posting error_menu opens no menu'
Check ((ErrEval 15) -match 'err15:true') 'error page: no "Not secure" on a normal https page'
Check ((ErrEval 17) -match 'err17:Blocked by the Shields\|.*Allow this site') 'error page: a blocked tracker opened as a page shows the Shields page'
Check ((ErrEval 18) -match 'err18:Offline') 'error page: offline variant'
Check ((ErrEval 19) -match 'err19:The computer.{1,3}s clock is wrong\|Continue anyway') 'error page: wrong clock variant'
Check ((ErrEval 20) -match 'err20:No secure version\|Continue over http') 'error page: http only variant'
Check ((ErrEval 21) -match 'err21:Site unreachable\|true\|1"') 'error page: one scene per kind (the black hole for "Site unreachable")'
function Named($name) { [string]($all | Where-Object { $_ -match "-> `"${name}:" } | Select-Object -First 1) }
Check ((Named dl1) -match 'dl1:bh-dl-page' -and (Named dl2) -match 'dl2:bh-dl-page\|/dlpage') 'download: a link that downloads a file leaves its page in place (no "Site unreachable")'
Check ((Named dl3) -match 'dl3:bh-dl-page\|/dlpage') 'download: the page is still there once the hidden sidebar has slid back'
Check ((Named cm2) -match 'cm2:.*Reload/Add this page to the Easel/Capture a live area to an Easel[^/]*/Watch an area for changes[^/]*/Copy page link/Find in page/Print[^/]*/Inspect page') 'page menu: a right click on the page shows BlackHole''s menu (Easel, live area, watched area)'
Check ((Named cm4) -match 'cm4:Open image in new tab/Copy image address') 'page menu: an image adds its own entries'
Check ((Named cm6) -notmatch 'menuing') 'page menu: a page with a menu of its own keeps it'
Check ((Named cm8) -match 'menuing.*\|true') 'page menu: a video the page plays by itself (a preview) gets it too'
Check ((Named cm10) -notmatch 'menuing') 'page menu: a player with the engine controls keeps the engine menu'
Check ((Named sb1) -match 'sb1:true\|[3-9]\d*' -and (Named sb2) -match 'sb2:false') 'sidebar: a counter that moves keeps the rows (no flicker), a changed tab redraws them'
Check ((Named sb3) -match 'sb3:true,false,true,false,false\|Idle') 'folders: a closed folder shows the pages open under it only (like Arc), a folder with none is dimmed'
# The test file landed in the real Downloads folder: removed.
$dlDir = (New-Object -ComObject Shell.Application).NameSpace('shell:Downloads').Self.Path
Get-ChildItem $dlDir -Filter 'bh-test-download*.zip' -ErrorAction SilentlyContinue | Where-Object { $_.Length -eq 4000 } | Remove-Item -Force
Check ($saved.Count -eq 40) "screenshots saved ($($saved.Count)/40)"
if ($fail) { Write-Host "evals:"; $evals }
exit [int]$fail
