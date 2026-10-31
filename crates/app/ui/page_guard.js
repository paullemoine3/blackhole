// Injected at document start in every frame of a page tab. The native engine exposes no
// network-level hook, so the tracker blocklist is enforced on the requests a page can make.
(() => {
  const BLOCKED = new Set(__BLOCKED_HOSTS__);
  const post = (m) => {
    try {
      window.ipc && window.ipc.postMessage(JSON.stringify(m));
    } catch (_) {}
  };
  const isBlocked = (raw) => {
    let host;
    try {
      host = new URL(String(raw), location.href).hostname.toLowerCase();
    } catch (_) {
      return false;
    }
    for (;;) {
      if (BLOCKED.has(host)) return true;
      const rest = host.slice(host.indexOf('.') + 1);
      if (host.indexOf('.') < 0 || rest.indexOf('.') < 0) return false;
      host = rest;
    }
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
  const sweep = (node) => {
    if (node.nodeType !== 1) return;
    const attr = TAGS[node.tagName];
    if (attr && node.getAttribute(attr) && isBlocked(node.getAttribute(attr)) && deny(node.getAttribute(attr))) node.remove();
    node.querySelectorAll && node.querySelectorAll(Object.keys(TAGS).join(',')).forEach(sweep);
  };
  new MutationObserver((records) => records.forEach((r) => r.addedNodes.forEach(sweep))).observe(
    document,
    { childList: true, subtree: true },
  );

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
