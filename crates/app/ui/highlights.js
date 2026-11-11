// Highlights on pages. The shell keeps the highlights (text and a little context) and hands this
// script the ones of the page shown; they are found again in the text and painted with the CSS
// Custom Highlight API: no node of the page is touched, and nothing is asked of the page.
// Nothing runs until the shell sends a highlight or asks for the selection.
(() => {
  if (window.top !== window || window.__bhMarks) return;
  const HL = !!(window.CSS && CSS.highlights && window.Highlight);
  const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'TEXTAREA', 'SELECT']);
  const CONTEXT = 48;
  // After the page settles, late content (lazy lists, single-page apps) is searched again for
  // the highlights not found yet, for this long at most.
  const WATCH_FOR = 30000;
  let marks = [];
  const ranges = new Map();
  let wanted = 0;
  let styled = false;
  let observer = null;
  let timer = 0;
  let watchEnd = 0;

  const style = () => {
    if (styled || !HL) return;
    styled = true;
    const css =
      '::highlight(bh-mark){background-color:rgba(255,213,79,.42);color:inherit}' +
      '::highlight(bh-mark-note){background-color:rgba(255,213,79,.42);color:inherit;text-decoration:underline dotted;text-decoration-thickness:2px}';
    try {
      const sheet = new CSSStyleSheet();
      sheet.replaceSync(css);
      document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
    } catch (_) {
      const node = document.createElement('style');
      node.textContent = css;
      (document.head || document.documentElement).appendChild(node);
    }
  };

  // The page's text as one string, with where each text node starts in it.
  const index = () => {
    const nodes = [];
    const starts = [];
    let text = '';
    if (!document.body) return { nodes, starts, text };
    const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) => (n.parentElement && !SKIP.has(n.parentElement.tagName) ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT),
    });
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      nodes.push(n);
      starts.push(text.length);
      text += n.data;
    }
    return { nodes, starts, text };
  };
  const point = (idx, offset, end) => {
    let lo = 0;
    let hi = idx.starts.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (idx.starts[mid] <= offset) lo = mid;
      else hi = mid - 1;
    }
    // An end that falls on the seam of two nodes belongs to the first.
    if (end && lo > 0 && idx.starts[lo] === offset) lo -= 1;
    return [idx.nodes[lo], offset - idx.starts[lo]];
  };
  const squeeze = (s) => s.replace(/\s+/g, '');
  const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  // Where a highlight is in the text: its words, whatever the white space between them (the
  // selection has line breaks where the page has none), nearest to its remembered context.
  const locate = (idx, mark) => {
    const words = mark.quote.split(' ').filter(Boolean);
    if (!words.length) return null;
    let pattern;
    try {
      pattern = new RegExp(words.map(escape).join('\\s*'), 'g');
    } catch (_) {
      return null;
    }
    const before = squeeze(mark.prefix || '');
    const after = squeeze(mark.suffix || '');
    let best = null;
    let bestScore = -1;
    let tries = 0;
    for (let m = pattern.exec(idx.text); m && tries < 200; m = pattern.exec(idx.text), tries++) {
      if (!m[0].length) {
        pattern.lastIndex++;
        continue;
      }
      const start = m.index;
      const end = start + m[0].length;
      let score = 0;
      if (before && squeeze(idx.text.slice(Math.max(0, start - 4 * CONTEXT), start)).endsWith(before)) score += 2;
      if (after && squeeze(idx.text.slice(end, end + 4 * CONTEXT)).startsWith(after)) score += 2;
      if (score > bestScore) {
        best = [start, end];
        bestScore = score;
      }
      if (score === 4) break;
    }
    if (!best) return null;
    const range = document.createRange();
    try {
      range.setStart(...point(idx, best[0], false));
      range.setEnd(...point(idx, best[1], true));
    } catch (_) {
      return null;
    }
    return range;
  };

  const paint = () => {
    if (!HL) return;
    CSS.highlights.delete('bh-mark');
    CSS.highlights.delete('bh-mark-note');
    const plain = [];
    const noted = [];
    for (const mark of marks) {
      const range = ranges.get(mark.id);
      if (range) (mark.note ? noted : plain).push(range);
    }
    if (plain.length) CSS.highlights.set('bh-mark', new Highlight(...plain));
    if (noted.length) CSS.highlights.set('bh-mark-note', new Highlight(...noted));
  };
  const reveal = () => {
    const range = wanted && ranges.get(wanted);
    if (!range) return;
    wanted = 0;
    const box = range.getBoundingClientRect();
    scrollTo({ top: Math.max(0, scrollY + box.top - innerHeight / 3), behavior: 'smooth' });
  };
  const unwatch = () => {
    if (observer) observer.disconnect();
    observer = null;
    clearTimeout(timer);
    timer = 0;
  };
  const anchor = () => {
    timer = 0;
    for (const id of [...ranges.keys()]) {
      const range = ranges.get(id);
      if (!marks.some((m) => m.id === id) || !range.startContainer.isConnected || range.collapsed) ranges.delete(id);
    }
    const missing = marks.filter((m) => !ranges.has(m.id));
    if (missing.length) {
      const idx = index();
      for (const mark of missing) {
        const range = locate(idx, mark);
        if (range) ranges.set(mark.id, range);
      }
    }
    paint();
    reveal();
    // Late content: look again when the page changes, while a highlight is still missing.
    const left = marks.some((m) => !ranges.has(m.id));
    if (!left || Date.now() > watchEnd) {
      unwatch();
    } else if (!observer && document.body) {
      observer = new MutationObserver(() => {
        if (!timer) timer = setTimeout(anchor, 1000);
      });
      observer.observe(document.body, { childList: true, subtree: true });
    }
  };
  const schedule = () => {
    if (!marks.length) {
      unwatch();
      ranges.clear();
      paint();
      return;
    }
    style();
    watchEnd = Date.now() + WATCH_FOR;
    clearTimeout(timer);
    timer = setTimeout(anchor, document.readyState === 'loading' ? 400 : 0);
  };
  addEventListener('load', () => marks.length && schedule());

  window.__bhMarks = {
    // The highlights of this page: [{id, quote, prefix, suffix, note}].
    set(list) {
      marks = Array.isArray(list) ? list : [];
      schedule();
    },
    // Scrolls to a highlight (once it is found).
    show(id) {
      wanted = id;
      reveal();
    },
    // The selection as a highlight to be: its text and the text around it.
    capture() {
      const sel = getSelection();
      if (!sel || sel.isCollapsed || !sel.rangeCount || !document.body) return null;
      const range = sel.getRangeAt(0);
      const quote = String(sel).slice(0, 4000);
      if (!quote.trim()) return null;
      let prefix = '';
      let suffix = '';
      try {
        const before = document.createRange();
        before.setStart(document.body, 0);
        before.setEnd(range.startContainer, range.startOffset);
        prefix = String(before).slice(-4 * CONTEXT);
        const last = sel.getRangeAt(sel.rangeCount - 1);
        const after = document.createRange();
        after.setStart(last.endContainer, last.endOffset);
        after.setEnd(document.body, document.body.childNodes.length);
        suffix = String(after).slice(0, 4 * CONTEXT);
      } catch (_) {}
      return { quote, prefix, suffix, title: document.title.slice(0, 300) };
    },
    // The highlight under a point of the window (0: none).
    at(x, y) {
      for (const [id, range] of ranges) {
        for (const r of range.getClientRects()) {
          if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return id;
        }
      }
      return 0;
    },
  };
})();
