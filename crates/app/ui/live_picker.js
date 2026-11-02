// Injected on demand in the page in front: lets the user click the area to capture live.
(() => {
  if (window.__bhPick) return;
  const box = document.createElement('div');
  box.style.cssText =
    'position:fixed;z-index:2147483647;pointer-events:none;border:2px solid #7c5cff;background:rgba(124,92,255,.18);border-radius:3px;display:none';
  const tip = document.createElement('div');
  tip.textContent = 'Click the area to capture live · Esc to cancel';
  tip.style.cssText =
    'position:fixed;z-index:2147483647;left:50%;top:12px;transform:translateX(-50%);background:#16151f;color:#fff;font:13px system-ui;padding:6px 12px;border-radius:8px;pointer-events:none';
  document.documentElement.append(box, tip);
  let target = null;
  const esc = (s) => (window.CSS && CSS.escape ? CSS.escape(s) : s.replace(/[^\w-]/g, '\\$&'));
  const selectorOf = (el) => {
    const parts = [];
    for (let n = el; n && n.nodeType === 1 && n !== document.documentElement; n = n.parentElement) {
      if (n.id && document.querySelectorAll('#' + esc(n.id)).length === 1) {
        parts.unshift('#' + esc(n.id));
        break;
      }
      const tag = n.tagName.toLowerCase();
      if (tag === 'body') {
        parts.unshift('body');
        break;
      }
      let i = 1;
      for (let p = n.previousElementSibling; p; p = p.previousElementSibling) if (p.tagName === n.tagName) i++;
      parts.unshift(`${tag}:nth-of-type(${i})`);
    }
    return parts.join(' > ');
  };
  const stop = () => {
    removeEventListener('mousemove', move, true);
    removeEventListener('click', click, true);
    removeEventListener('keydown', key, true);
    box.remove();
    tip.remove();
    delete window.__bhPick;
  };
  const move = (e) => {
    const el = e.target;
    if (!el || el === box || el === tip) return;
    target = el;
    const r = el.getBoundingClientRect();
    Object.assign(box.style, { display: 'block', left: r.left + 'px', top: r.top + 'px', width: r.width + 'px', height: r.height + 'px' });
  };
  const click = (e) => {
    e.preventDefault();
    e.stopPropagation();
    const el = target || e.target;
    const r = el.getBoundingClientRect();
    const selector = selectorOf(el);
    stop();
    try {
      window.ipc.postMessage(JSON.stringify({ cmd: 'live_pick', selector, w: r.width, h: r.height }));
    } catch (_) {}
  };
  const key = (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      stop();
    }
  };
  addEventListener('mousemove', move, true);
  addEventListener('click', click, true);
  addEventListener('keydown', key, true);
  window.__bhPick = true;
})();
