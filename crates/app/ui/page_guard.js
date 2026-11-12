// Injected at document start in every frame of a page tab. Where the engine cannot block
// requests itself (WebView2), the blocklist is enforced here, on the requests a page can make.
// On macOS the list is empty (WebKit content rule lists do the blocking, see
// `views::block_natively`) and none of the page's functions is replaced: wrapping
// `setAttribute`, `fetch` or the `src` setters costs on every call of a heavy web app.
(() => {
  const BLOCKED = new Set(__BLOCKED_HOSTS__);
  const post = (m) => {
    try {
      window.__bhipc && window.__bhipc.postMessage(JSON.stringify(m));
    } catch (_) {}
  };
  // Pages set thousands of URLs: decisions are cached per host and URLs are only parsed when
  // they are not a plain relative path.
  const verdicts = new Map();
  const hostBlocked = (start) => {
    let host = start;
    for (;;) {
      if (BLOCKED.has(host)) return true;
      const rest = host.slice(host.indexOf('.') + 1);
      if (host.indexOf('.') < 0 || rest.indexOf('.') < 0) return false;
      host = rest;
    }
  };
  const isBlocked = (raw) => {
    if (!BLOCKED.size) return false;
    const text = String(raw);
    if (text.charCodeAt(0) === 47 && text.charCodeAt(1) !== 47) return false; // "/path"
    if (text.startsWith('data:') || text.startsWith('blob:') || text.startsWith('#')) return false;
    let host;
    try {
      host = new URL(text, location.href).hostname.toLowerCase();
    } catch (_) {
      return false;
    }
    let verdict = verdicts.get(host);
    if (verdict === undefined) {
      verdict = hostBlocked(host);
      verdicts.set(host, verdict);
    }
    return verdict;
  };
  const deny = (raw) => {
    let host = '';
    try {
      host = new URL(String(raw), location.href).hostname;
    } catch (_) {}
    post({ cmd: 'blocked', host });
    return true;
  };

  if (BLOCKED.size) {
  const nativeFetch = window.fetch;
  window.fetch = function (input) {
    const url = typeof input === 'object' && input && 'url' in input ? input.url : input;
    if (url && isBlocked(url) && deny(url)) return Promise.reject(new TypeError('Blocked by BlackHole'));
    return nativeFetch.apply(this, arguments);
  };

  const xhrOpen = XMLHttpRequest.prototype.open;
  const xhrSend = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function (method, url) {
    this.__bhBlocked = isBlocked(url);
    this.__bhUrl = url;
    return xhrOpen.apply(this, arguments);
  };
  XMLHttpRequest.prototype.send = function () {
    if (this.__bhBlocked && deny(this.__bhUrl)) {
      setTimeout(() => this.dispatchEvent(new ProgressEvent('error')));
      return;
    }
    return xhrSend.apply(this, arguments);
  };

  if (navigator.sendBeacon) {
    const beacon = navigator.sendBeacon.bind(navigator);
    navigator.sendBeacon = (url, data) => (isBlocked(url) && deny(url) ? false : beacon(url, data));
  }

  const TAGS = { SCRIPT: 'src', IMG: 'src', IFRAME: 'src', EMBED: 'src', SOURCE: 'src', LINK: 'href' };
  for (const [tag, attr] of Object.entries(TAGS)) {
    const ctor = window['HTML' + tag[0] + tag.slice(1).toLowerCase() + 'Element'];
    const desc = ctor && Object.getOwnPropertyDescriptor(ctor.prototype, attr);
    if (!desc || !desc.set) continue;
    Object.defineProperty(ctor.prototype, attr, {
      ...desc,
      set(value) {
        if (isBlocked(value) && deny(value)) return;
        desc.set.call(this, value);
      },
    });
  }
  const setAttribute = Element.prototype.setAttribute;
  Element.prototype.setAttribute = function (name, value) {
    if (TAGS[this.tagName] === String(name).toLowerCase() && isBlocked(value) && deny(value)) return;
    return setAttribute.call(this, name, value);
  };

  // Elements written by the HTML parser never go through the setters above.
  const SELECTOR = Object.keys(TAGS).join(',');
  const check = (node) => {
    const attr = TAGS[node.tagName];
    const value = attr && node.getAttribute(attr);
    if (value && isBlocked(value) && deny(value)) node.remove();
  };
  const sweep = (node) => {
    if (node.nodeType !== 1) return;
    check(node);
    if (node.firstElementChild) node.querySelectorAll(SELECTOR).forEach(check);
  };
  new MutationObserver((records) => {
    for (const r of records) for (const n of r.addedNodes) sweep(n);
  }).observe(document, { childList: true, subtree: true });
  }

  if (window.top !== window) return;
  // Shift+click previews a link in a floating window (Arc's "Peek").
  addEventListener(
    'click',
    (e) => {
      const link = e.shiftKey && e.target.closest && e.target.closest('a[href]');
      if (!link || !/^https?:/.test(link.href)) return;
      e.preventDefault();
      e.stopPropagation();
      post({ cmd: 'peek', url: link.href });
    },
    true,
  );

  // The link under the pointer, through shadow roots (YouTube's titles live in custom elements).
  const linkAt = (e) => {
    const a = e.composedPath ? e.composedPath().find((n) => n && n.tagName === 'A' && n.href) : null;
    return a && /^https?:/.test(a.href) ? a : null;
  };
  const mac = /mac/i.test(navigator.platform);
  // Middle click, or Ctrl/Cmd+click, on a link: a new tab in the background, like other
  // browsers. Caught before the page, which may handle clicks itself (single-page apps).
  addEventListener(
    'auxclick',
    (e) => {
      const link = e.button === 1 && linkAt(e);
      if (!link) return;
      e.preventDefault();
      e.stopPropagation();
      post({ cmd: 'open_link', url: link.href });
    },
    true,
  );
  addEventListener(
    'mousedown',
    (e) => {
      if (e.button === 1 && linkAt(e)) e.preventDefault(); // no autoscroll over a link
    },
    true,
  );
  addEventListener(
    'click',
    (e) => {
      if (e.button !== 0 || e.shiftKey || e.altKey || !(mac ? e.metaKey : e.ctrlKey)) return;
      const link = linkAt(e);
      if (!link) return;
      e.preventDefault();
      e.stopPropagation();
      post({ cmd: 'open_link', url: link.href });
    },
    true,
  );
  // Pilot split: while the shell says this page drives the other pane (`__bhPilot`), a plain
  // click on a link to another page is handed to the shell instead of being followed here.
  addEventListener(
    'click',
    (e) => {
      if (!window.__bhPilot || e.button !== 0 || e.shiftKey || e.altKey || e.metaKey || e.ctrlKey) return;
      const link = linkAt(e);
      if (!link || link.hasAttribute('download')) return;
      if (link.href.split('#')[0] === location.href.split('#')[0]) return;
      e.preventDefault();
      e.stopPropagation();
      post({ cmd: 'pilot_link', url: link.href });
    },
    true,
  );
  // The selected text under the pointer (the shell bounds and cleans it again).
  const selectionAt = (e) => {
    const sel = getSelection();
    if (!sel || sel.isCollapsed) return '';
    for (let i = 0; i < sel.rangeCount; i++) {
      for (const r of sel.getRangeAt(i).getClientRects()) {
        if (e.clientX >= r.left - 2 && e.clientX <= r.right + 2 && e.clientY >= r.top - 2 && e.clientY <= r.bottom + 2) {
          return String(sel).slice(0, 1000);
        }
      }
    }
    return '';
  };
  const editable = (e) =>
    e.composedPath().some((n) => n && (n.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(n.tagName)));
  // Right click on a link, or on selected text: the browser's own menu (new tab, new window…,
  // search the selection). Shift+right click, a text field (paste, spelling) and frames keep
  // the engine's menu.
  addEventListener(
    'contextmenu',
    (e) => {
      if (e.shiftKey || editable(e)) return;
      const link = linkAt(e);
      const text = selectionAt(e);
      // A highlight under the pointer (highlights.js) has its own entries in the menu.
      let mark = 0;
      try {
        mark = (window.__bhMarks && window.__bhMarks.at(e.clientX, e.clientY)) || 0;
      } catch (_) {}
      if (!link && !text.trim() && !mark) return;
      e.preventDefault();
      e.stopPropagation();
      post({ cmd: 'link_menu', url: link ? link.href : '', text, mark, x: e.clientX, y: e.clientY });
    },
    true,
  );
  // Right click anywhere else on the page: BlackHole's page menu (back, reload, Easel, live
  // capture, watch an area…). Asked last and only if the page did not show a menu of its own
  // (Docs, Sheets, Figma); a video or a sound keeps the engine's menu (loop, controls, save).
  window.addEventListener('contextmenu', (e) => {
    if (e.defaultPrevented || e.shiftKey || editable(e)) return;
    const path = e.composedPath();
    if (path.some((n) => n && /^(VIDEO|AUDIO|CANVAS|EMBED|OBJECT)$/.test(n.tagName))) return;
    const img = path.find((n) => n && n.tagName === 'IMG' && (n.currentSrc || n.src));
    const src = img ? img.currentSrc || img.src : '';
    e.preventDefault();
    post({ cmd: 'link_menu', url: '', text: '', mark: 0, image: /^https?:/.test(src) ? src.slice(0, 4000) : '', page: true, x: e.clientX, y: e.clientY });
  });

  // Single-page apps change the URL without a navigation; tell the shell to re-read it.
  const report = () => post({ cmd: 'location' });
  for (const method of ['pushState', 'replaceState']) {
    const original = history[method];
    history[method] = function () {
      const result = original.apply(this, arguments);
      report();
      return result;
    };
  }
  addEventListener('popstate', report);
  addEventListener('hashchange', report);
})();
