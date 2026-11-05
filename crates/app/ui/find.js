// "Find in page" bar (Cmd/Ctrl+F). The native engines expose no find UI to an embedding app,
// so the bar lives in the page and relies on the engines' `window.find`.
(() => {
  if (window.top !== window || window.__bhFind) return;
  let host = null;
  let input = null;
  let info = null;

  const run = (backwards) => {
    const text = input.value;
    if (!text) {
      info.textContent = '';
      return;
    }
    const found = window.find(text, false, backwards, true, false, false, false);
    info.textContent = found ? '' : 'No match';
  };
  const close = () => {
    if (!host) return;
    host.remove();
    host = input = info = null;
    getSelection().removeAllRanges();
  };
  const build = () => {
    host = document.createElement('div');
    host.style.cssText = 'all:initial;position:fixed;top:12px;right:16px;z-index:2147483647';
    const root = host.attachShadow({ mode: 'closed' });
    root.innerHTML = `<style>
      .bar{display:flex;align-items:center;gap:6px;padding:6px 8px;border-radius:10px;background:rgba(32,32,40,.96);
        box-shadow:0 6px 24px rgba(0,0,0,.45),0 0 0 1px rgba(255,255,255,.12);font:13px -apple-system,Segoe UI,sans-serif;color:#eee}
      input{width:190px;background:rgba(255,255,255,.1);border:0;border-radius:6px;padding:5px 8px;color:#fff;font:inherit;outline:none}
      button{background:none;border:0;color:#ddd;font-size:15px;cursor:pointer;border-radius:5px;padding:2px 7px}
      button:hover{background:rgba(255,255,255,.14)}
      .n{min-width:52px;color:#ff9a9a;font-size:12px}
    </style><div class="bar"><input placeholder="Find in page" spellcheck="false"><span class="n"></span>
      <button data-d="1" title="Previous">↑</button><button data-d="0" title="Next">↓</button><button data-x title="Close">✕</button></div>`;
    input = root.querySelector('input');
    info = root.querySelector('.n');
    input.addEventListener('input', () => {
      getSelection().removeAllRanges();
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
    const selected = String(getSelection());
    if (!host) build();
    if (selected && selected.length < 100 && !selected.includes('\n')) input.value = selected;
    input.focus();
    input.select();
  };
})();
