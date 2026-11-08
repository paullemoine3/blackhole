// "Find in page" bar (Cmd/Ctrl+F). The native engines expose no find UI to an embedding app,
// so the bar lives in the page. Matches are painted with the CSS Custom Highlight API, which
// leaves the selection and the focus alone: typing goes on in the bar. Engines without it fall
// back to `window.find`, which selects the match (and may take the focus, given back at once).
(() => {
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

  const collect = (text) => {
    const needle = text.toLowerCase();
    const out = [];
    if (!document.body) return out;
    const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) => {
        const p = n.parentElement;
        if (!p || SKIP.has(p.tagName)) return NodeFilter.FILTER_REJECT;
        if (p.checkVisibility && !p.checkVisibility()) return NodeFilter.FILTER_REJECT;
        return NodeFilter.FILTER_ACCEPT;
      },
    });
    for (let n = walk.nextNode(); n && out.length < MAX; n = walk.nextNode()) {
      const hay = n.data.toLowerCase();
      for (let i = hay.indexOf(needle); i !== -1 && out.length < MAX; i = hay.indexOf(needle, i + needle.length)) {
        const r = document.createRange();
        r.setStart(n, i);
        r.setEnd(n, i + needle.length);
        out.push(r);
      }
    }
    return out;
  };
  const clear = () => {
    if (HL) {
      CSS.highlights.delete('bh-find');
      CSS.highlights.delete('bh-find-cur');
    }
    ranges = [];
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
      show(found ? '' : 'No match', found);
      return;
    }
    if (text !== last) {
      ranges = collect(text);
      last = text;
      at = backwards ? ranges.length - 1 : 0;
    } else if (ranges.length) {
      at = (at + (backwards ? -1 : 1) + ranges.length) % ranges.length;
    }
    if (!ranges.length) {
      CSS.highlights.delete('bh-find');
      CSS.highlights.delete('bh-find-cur');
      show('No match', false);
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
    root.innerHTML = `<style>
      .bar{display:flex;align-items:center;gap:6px;padding:6px 8px;border-radius:10px;background:rgba(32,32,40,.96);
        box-shadow:0 6px 24px rgba(0,0,0,.45),0 0 0 1px rgba(255,255,255,.12);font:13px -apple-system,Segoe UI,sans-serif;color:#eee}
      input{width:190px;background:rgba(255,255,255,.1);border:0;border-radius:6px;padding:5px 8px;color:#fff;font:inherit;outline:none}
      button{background:none;border:0;color:#ddd;font-size:15px;cursor:pointer;border-radius:5px;padding:2px 7px}
      button:hover{background:rgba(255,255,255,.14)}
      .n{min-width:52px;font-size:12px;text-align:right}
    </style><div class="bar"><input placeholder="Find in page" spellcheck="false"><span class="n"></span>
      <button data-d="1" title="Previous">↑</button><button data-d="0" title="Next">↓</button><button data-x title="Close">✕</button></div>`;
    input = root.querySelector('input');
    info = root.querySelector('.n');
    input.addEventListener('input', () => {
      if (!HL) getSelection().removeAllRanges();
      last = '';
      run(false);
    });
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
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
