(function () {
  if (window.__bhLog) return;
  window.__bhLog = true;
  const send = (m) => {
    try {
      window.__bhipc && window.__bhipc.postMessage(JSON.stringify({ cmd: 'debug_log', msg: location.host + ' @' + (Date.now() % 100000) + ' ' + m }));
    } catch (e) {}
  };
  const show = (a) => {
    try {
      return a instanceof Error ? a.stack || a.message : typeof a === 'string' ? a : JSON.stringify(a);
    } catch (e) {
      return String(a);
    }
  };
  ['error', 'warn'].forEach((k) => {
    const o = console[k];
    console[k] = function () {
      send('console.' + k + ' ' + Array.from(arguments).map(show).join(' '));
      return o.apply(this, arguments);
    };
  });
  window.addEventListener('error', (e) => send('error ' + e.message + ' @' + e.filename + ':' + e.lineno));
  window.addEventListener('unhandledrejection', (e) => send('rejection ' + show(e.reason)));
  const describe = (n) =>
    n.tagName + ' type=' + n.getAttribute('type') + ' nomodule=' + n.hasAttribute('nomodule') + ' src=' + (n.getAttribute('src') || '').slice(-90);
  new MutationObserver((records) => {
    for (const r of records)
      for (const n of r.addedNodes)
        if (n.tagName === 'SCRIPT' && /early-entry|entry/.test(n.getAttribute('src') || '')) send('script+ ' + describe(n));
  }).observe(document, { childList: true, subtree: true });
  addEventListener('error', (e) => e.target && e.target.tagName === 'SCRIPT' && send('script load error ' + describe(e.target)), true);
  ['dragstart', 'drop', 'dragend'].forEach((k) =>
    addEventListener(k, (e) => send(k + ' ' + (e.target && e.target.tagName) + ' effect=' + (e.dataTransfer && e.dataTransfer.dropEffect) + ' cancelled=' + e.defaultPrevented), true)
  );
  let lastOver = 0;
  ['dragenter', 'dragover'].forEach((k) =>
    addEventListener(k, (e) => {
      const now = Date.now();
      if (k === 'dragover' && now - lastOver < 700) return;
      lastOver = now;
      setTimeout(() => send(k + ' ' + (e.target && e.target.tagName) + ' prevented=' + e.defaultPrevented + ' effectAllowed=' + (e.dataTransfer && e.dataTransfer.effectAllowed) + ' types=' + (e.dataTransfer && Array.from(e.dataTransfer.types).join(','))), 0);
    }, true)
  );
  const f = window.fetch;
  if (f) {
    window.fetch = function () {
      const u = String(arguments[0] && arguments[0].url ? arguments[0].url : arguments[0]);
      return f.apply(this, arguments).then(
        (r) => {
          if (!r.ok) send('fetch ' + r.status + ' ' + u);
          return r;
        },
        (e) => {
          send('fetch FAIL ' + u + ' ' + show(e));
          throw e;
        }
      );
    };
  }
})();
