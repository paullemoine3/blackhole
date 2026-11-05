// Reports the page's own icon so the sidebar and the home page can show it. Nothing is fetched
// here: the URL comes from the page's <link rel="icon"> (or its /favicon.ico).
(() => {
  if (!/^https?:$/.test(location.protocol)) return;
  let last = '';
  let timer = 0;
  const report = () => {
    timer = 0;
    const links = [...document.querySelectorAll('link[rel~="icon" i]')];
    const link = links.find((l) => l.href) || null;
    const href = link ? link.href : location.origin + '/favicon.ico';
    if (href === last) return;
    last = href;
    try {
      window.__bhipc && window.__bhipc.postMessage(JSON.stringify({ cmd: 'favicon', url: href }));
    } catch (_) {}
  };
  const schedule = () => {
    if (!timer) timer = setTimeout(report, 400);
  };
  document.addEventListener('DOMContentLoaded', () => {
    schedule();
    new MutationObserver(schedule).observe(document.head || document.documentElement, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['href', 'rel'],
    });
  });
  addEventListener('load', schedule);
})();
