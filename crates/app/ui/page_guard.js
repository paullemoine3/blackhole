// Injected at document start in every frame of a page tab. The native engine exposes no
// network-level hook, so the tracker blocklist is enforced on the requests a page can make.
(() => {
  const BLOCKED = new Set(__BLOCKED_HOSTS__);
  const post = (m) => {
    try {
      window.ipc && window.ipc.postMessage(JSON.stringify(m));
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
  if (BLOCKED.size) {
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
