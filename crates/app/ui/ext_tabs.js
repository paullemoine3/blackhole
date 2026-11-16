// Extension popups (Windows). WebView2 makes every view a tab in a window of its own, the popup
// included: asked for "the active tab of the current window", it answers with the popup itself
// (chrome-extension://…), so uBlock Origin Lite or Ghostery say they are not on a web page.
// Such a question gets the tab of the page in front instead: the address BlackHole gives
// (`__BH_PAGE__`, empty on the home page), else the web page seen last.
(() => {
  if (window.top !== window) return;
  const page = __BH_PAGE__;
  const tabs = globalThis.chrome && chrome.tabs;
  if (!page || !tabs || typeof tabs.query !== 'function') return;
  const query = tabs.query.bind(tabs);
  const own = (t) => (t.url || t.pendingUrl || '').startsWith(location.origin + '/');
  const web = (t) => /^https?:/i.test(t.url || '');
  const front = async () => {
    const pages = (await query({})).filter((t) => !own(t) && web(t));
    const same = pages.filter((t) => t.url === page);
    const pool = same.length ? same : pages;
    pool.sort((a, b) => (b.lastAccessed || 0) - (a.lastAccessed || 0));
    return pool.slice(0, 1);
  };
  const patched = function (info, callback) {
    const asksFront =
      info && info.active === true && (info.currentWindow === true || info.lastFocusedWindow === true);
    const answer = asksFront
      ? query(info).then(async (found) => {
          if (found.length && !found.every(own)) return found;
          const tab = await front();
          return tab.length ? tab : found;
        })
      : query(info);
    if (typeof callback !== 'function') return answer;
    answer.then(callback, () => callback([]));
  };
  try {
    Object.defineProperty(tabs, 'query', { value: patched, configurable: true, writable: true });
  } catch (_) {
    try { tabs.query = patched; } catch (_) { /* the popup keeps the engine's answer */ }
  }
})();
