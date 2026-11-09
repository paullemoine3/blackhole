// Document-start script of the tab pages (Android). Gives our error page the same
// `window.__bhipc.postMessage` as on the desktop. `bhPage` is the WebMessageListener the
// shell adds to tab views: any page can reach it, so the core only accepts `PageEvent`s
// from it (untrusted) and checks the tab really shows our error page.
(() => {
  if (window.top !== window || window.__bhipc) return;
  const port = window.bhPage;
  if (!port || typeof port.postMessage !== 'function') return;
  const post = port.postMessage.bind(port);
  Object.defineProperty(window, '__bhipc', {
    value: Object.freeze({ postMessage: (s) => post(String(s)) }),
  });
})();
