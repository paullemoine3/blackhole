// Tells the shell about the page's requests to other hosts, so that trackers on no list can be
// learned (`domain::trackers`: the shell decides what looks like tracking, from several sites).
// Passive: it reads the resource timings the engine keeps anyway, through a PerformanceObserver;
// no function of the page is replaced. Bounded: three addresses per host, 200 in all, sent in
// batches at most every two seconds.
(() => {
  if (!/^https?:$/.test(location.protocol) || !window.PerformanceObserver) return;
  const page = location.hostname;
  const perHost = new Map();
  let total = 0, queue = [], timer = 0;
  const flush = () => {
    timer = 0;
    if (!queue.length) return;
    try {
      window.__bhipc && window.__bhipc.postMessage(JSON.stringify({ cmd: 'requests', list: queue.splice(0, 50) }));
    } catch (_) {}
    if (queue.length) timer = setTimeout(flush, 2000);
  };
  const seen = (entries) => {
    for (const e of entries) {
      if (total >= 200) return;
      let u;
      try { u = new URL(e.name); } catch (_) { continue; }
      if (!/^https?:$/.test(u.protocol) || u.hostname === page) continue;
      const n = perHost.get(u.hostname) || 0;
      if (n >= 3) continue;
      perHost.set(u.hostname, n + 1);
      total++;
      queue.push({ url: e.name.slice(0, 400), kind: String(e.initiatorType || '').slice(0, 20) });
    }
    if (queue.length && !timer) timer = setTimeout(flush, 2000);
  };
  // An extension that answers in place of a tracker (uBlock's stand-in scripts) leaves no timing:
  // the hosts of the script tags that were not heard of are told too, once each.
  const tags = () => {
    for (const s of document.scripts) {
      if (total >= 200) return;
      if (!s.src || (s.type && !/^(module|(text|application)\/(x-)?(java|ecma)script)$/i.test(s.type))) continue;
      let u;
      try { u = new URL(s.src); } catch (_) { continue; }
      if (!/^https?:$/.test(u.protocol) || u.hostname === page || perHost.has(u.hostname)) continue;
      perHost.set(u.hostname, 1);
      total++;
      queue.push({ url: s.src.slice(0, 400), kind: 'tag' });
    }
    if (queue.length && !timer) timer = setTimeout(flush, 2000);
  };
  if (__TAGS__) addEventListener('load', () => { setTimeout(tags, 1500); setTimeout(tags, 8000); });
  try {
    new PerformanceObserver((list) => seen(list.getEntries())).observe({ type: 'resource', buffered: true });
  } catch (_) {}
})();
