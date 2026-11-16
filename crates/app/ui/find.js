// "Find in page" bar (Cmd/Ctrl+F). The native engines expose no find UI to an embedding app,
// so the bar lives in the page. Matches are painted with the CSS Custom Highlight API, which
// leaves the selection and the focus alone: typing goes on in the bar. Engines without it fall
// back to `window.find`, which selects the match (and may take the focus, given back at once).
(() => {
  // Texts in the interface language, set when the page's view is created (views::page_script).
  const TEXT = __BH_TEXT__;
  const T = (key) => TEXT[key] || key;
  if (window.top !== window || window.__bhFind) return;
  const HL = !!(window.CSS && CSS.highlights && window.Highlight);
  const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'TEXTAREA', 'SELECT']);
  const MAX = 2000;
  let host = null;
  let input = null;
  let info = null;
  let ranges = [];
  let at = -1;
  let last = '';

  // A block boundary between two text nodes reads as a space, as in the engines' own find.
  const BLOCK = 'p,div,li,dt,dd,td,th,tr,h1,h2,h3,h4,h5,h6,section,article,header,footer,nav,aside,blockquote,pre,figcaption,label,option,br';
  // White space as `\s` sees it, by code unit (no regular expression per character).
  const isSpace = (c) => c <= 32 || c === 160 || c === 0x1680 || (c >= 0x2000 && c <= 0x200a) || c === 0x2028 || c === 0x2029 || c === 0x202f || c === 0x205f || c === 0x3000 || c === 0xfeff;
  // Every visible text node is read into one string, each run of white space (in a node or
  // across nodes) as a single space, with the node and offset of every character kept beside
  // it (typed arrays): a match can then cross a link, a `<b>` or a line break of the HTML
  // source. The index is kept while the bar is open and rebuilt only after the page changed
  // (`dirty`, from a MutationObserver): typing a word in a long CI log reads the page once.
  let index = null;
  let dirty = true;
  let watcher = null;
  const build_index = () => {
    const nodes = [];
    let cap = 1 << 16;
    let nodeOf = new Int32Array(cap);
    let offOf = new Int32Array(cap);
    const parts = [];
    let len = 0;
    let lastSpace = true;
    const grow = () => {
      cap *= 2;
      const n = new Int32Array(cap); n.set(nodeOf); nodeOf = n;
      const o = new Int32Array(cap); o.set(offOf); offOf = o;
    };
    const push = (s, ni, off) => {
      if (len === cap) grow();
      parts.push(s);
      nodeOf[len] = ni;
      offOf[len] = off;
      len++;
    };
    if (!document.body) return { hay: '', nodes, nodeOf, offOf };
    // Many text nodes share a parent: its visibility and its block are asked once.
    const shown = new Map();
    const blockOf = new Map();
    const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) => {
        const p = n.parentElement;
        if (!p || SKIP.has(p.tagName)) return NodeFilter.FILTER_REJECT;
        let v = shown.get(p);
        if (v === undefined) { v = !p.checkVisibility || p.checkVisibility(); shown.set(p, v); }
        return v ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_REJECT;
      },
    });
    let block = null;
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      const p = n.parentElement;
      let b = blockOf.get(p);
      if (b === undefined) { b = p.closest(BLOCK); blockOf.set(p, b); }
      const ni = nodes.length;
      nodes.push(n);
      if (ni > 0 && b !== block && !lastSpace) { push(' ', ni, 0); lastSpace = true; }
      block = b;
      const data = n.data;
      const lower = data.toLowerCase();
      if (lower.length === data.length) {
        // Usual case: one character for one. Runs of letters go in as one slice.
        let from = -1;
        for (let i = 0; i < data.length; i++) {
          if (isSpace(data.charCodeAt(i))) {
            if (from !== -1) { flush(lower, from, i, ni); from = -1; }
            if (!lastSpace) { push(' ', ni, i); lastSpace = true; }
          } else if (from === -1) from = i;
        }
        if (from !== -1) flush(lower, from, data.length, ni);
      } else {
        // A few characters grow when lower-cased (İ): one at a time, offsets stay the node's.
        for (let i = 0; i < data.length; i++) {
          if (isSpace(data.charCodeAt(i))) {
            if (!lastSpace) { push(' ', ni, i); lastSpace = true; }
          } else for (const c of data[i].toLowerCase()) { push(c, ni, i); lastSpace = false; }
        }
      }
    }
    function flush(lower, from, to, ni) {
      while (len + (to - from) > cap) grow();
      parts.push(lower.slice(from, to));
      for (let i = from; i < to; i++) { nodeOf[len] = ni; offOf[len] = i; len++; }
      lastSpace = false;
    }
    return { hay: parts.join(''), nodes, nodeOf, offOf };
  };
  const collect = (text, previous) => {
    const needle = text.toLowerCase().replace(/\s+/g, ' ').trim();
    if (!document.body || !needle) return { needle, starts: [] };
    if (dirty || !index) { index = build_index(); dirty = false; previous = null; }
    const { hay } = index;
    const starts = [];
    if (previous && needle.startsWith(previous.needle) && previous.starts.length < MAX) {
      // The word grew: only the places where the shorter one matched can still match.
      for (const i of previous.starts) if (hay.startsWith(needle, i)) starts.push(i);
    } else {
      for (let i = hay.indexOf(needle); i !== -1 && starts.length < MAX; i = hay.indexOf(needle, i + needle.length)) starts.push(i);
    }
    return { needle, starts };
  };
  const rangesOf = (found) => {
    const { nodes, nodeOf, offOf } = index;
    const n = found.needle.length;
    return found.starts.map((i) => {
      const end = i + n - 1;
      const r = document.createRange();
      r.setStart(nodes[nodeOf[i]], offOf[i]);
      r.setEnd(nodes[nodeOf[end]], offOf[end] + 1);
      return r;
    });
  };
  let found = null;
  const clear = () => {
    if (HL) {
      CSS.highlights.delete('bh-find');
      CSS.highlights.delete('bh-find-cur');
    }
    ranges = [];
    found = null;
    at = -1;
    last = '';
  };
  const show = (text, ok) => {
    info.textContent = text;
    info.style.color = ok ? '#bbb' : '#ff9a9a';
  };
  const run = (backwards) => {
    const text = input.value;
    if (!text) {
      clear();
      show('', true);
      return;
    }
    if (!HL) {
      const found = window.find(text, false, backwards, true, false, false, false);
      input.focus();
      show(found ? '' : T('page.no_match'), found);
      return;
    }
    if (text !== last) {
      found = collect(text, found);
      ranges = rangesOf(found);
      last = text;
      at = backwards ? ranges.length - 1 : 0;
    } else if (ranges.length) {
      at = (at + (backwards ? -1 : 1) + ranges.length) % ranges.length;
    }
    if (!ranges.length) {
      CSS.highlights.delete('bh-find');
      CSS.highlights.delete('bh-find-cur');
      show(T('page.no_match'), false);
      return;
    }
    CSS.highlights.set('bh-find', new Highlight(...ranges));
    CSS.highlights.set('bh-find-cur', new Highlight(ranges[at]));
    const el = ranges[at].startContainer.parentElement;
    if (el) el.scrollIntoView({ block: 'center', inline: 'nearest' });
    show(`${at + 1}/${ranges.length}${ranges.length === MAX ? '+' : ''}`, true);
  };
  const close = () => {
    if (!host) return;
    host.remove();
    host = input = info = null;
    if (watcher) watcher.disconnect();
    watcher = null;
    index = null;
    dirty = true;
    clear();
  };
  const build = () => {
    if (HL) {
      // Constructed sheets are not inline styles: page CSPs do not block them.
      try {
        const sheet = new CSSStyleSheet();
        sheet.replaceSync(
          '::highlight(bh-find){background-color:#ffe36e;color:#000}::highlight(bh-find-cur){background-color:#ff9632;color:#000}',
        );
        document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
      } catch (e) {}
    }
    host = document.createElement('div');
    host.style.cssText = 'all:initial;position:fixed;top:12px;right:16px;z-index:2147483647';
    const root = host.attachShadow({ mode: 'closed' });
    // No `innerHTML`: pages under Trusted Types reject it (see autofill.js).
    const CSS_TEXT = `      .bar{display:flex;align-items:center;gap:6px;padding:6px 8px;border-radius:10px;background:rgba(32,32,40,.96);
        box-shadow:0 6px 24px rgba(0,0,0,.45),0 0 0 1px rgba(255,255,255,.12);font:13px -apple-system,Segoe UI,sans-serif;color:#eee}
      input{width:190px;background:rgba(255,255,255,.1);border:0;border-radius:6px;padding:5px 8px;color:#fff;font:inherit;outline:none}
      button{background:none;border:0;color:#ddd;font-size:15px;cursor:pointer;border-radius:5px;padding:2px 7px;min-width:28px;min-height:28px}
      button:focus-visible{outline:2px solid #8ab4ff;outline-offset:1px}
      button:hover{background:rgba(255,255,255,.14)}
      .n{min-width:52px;font-size:12px;text-align:right}
    `;
    try {
      const sheet = new CSSStyleSheet();
      sheet.replaceSync(CSS_TEXT);
      root.adoptedStyleSheets = [sheet];
    } catch (e) {
      const style = document.createElement('style');
      style.textContent = CSS_TEXT;
      root.appendChild(style);
    }
    const node = (tag, attrs, text) => {
      const el = document.createElement(tag);
      for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v);
      if (text) el.textContent = text;
      return el;
    };
    const bar = node('div', { class: 'bar' });
    input = node('input', { placeholder: T('palette.find'), spellcheck: 'false' });
    info = node('span', { class: 'n' });
    bar.append(
      input,
      info,
      node('button', { 'data-d': '1', title: T('page.previous') }, '\u2191'),
      node('button', { 'data-d': '0', title: T('page.next') }, '\u2193'),
      node('button', { 'data-x': '', title: T('window.close') }, '\u2715'),
    );
    root.appendChild(bar);
    // Each search reads the whole page: wait for a pause in the typing.
    let typing = 0;
    input.addEventListener('input', () => {
      clearTimeout(typing);
      typing = setTimeout(() => {
        if (!input) return;
        if (!HL) getSelection().removeAllRanges();
        last = '';
        run(false);
      }, 80);
    });
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        clearTimeout(typing);
        run(e.shiftKey);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        close();
      }
    });
    root.querySelector('.bar').addEventListener('click', (e) => {
      const b = e.target.closest('button');
      if (!b) return;
      if ('x' in b.dataset) close();
      else run(b.dataset.d === '1');
    });
    document.documentElement.appendChild(host);
    // The page changed (a log that streams, a list that scrolls): the next search reads it again.
    try {
      watcher = new MutationObserver(() => { dirty = true; });
      watcher.observe(document.body || document.documentElement, { childList: true, subtree: true, characterData: true });
    } catch (e) {}
  };

  window.__bhFind = (action) => {
    if (action === 'next' || action === 'prev') {
      if (input) run(action === 'prev');
      return;
    }
    // Read before the bar exists: building it moves the focus.
    const selected = String(getSelection());
    if (!host) build();
    if (selected && selected.length < 100 && !selected.includes('\n')) {
      input.value = selected;
      last = '';
      run(false);
    }
    input.focus();
    input.select();
  };
})();
