// BlackHole's notices, injected in its own pages with icons.js: one component draws a notice
// (icon of its kind, text, buttons, the line of the time left) wherever a page hosts it: the
// sidebar's toast, or the top bar's address field while the sidebar is hidden. Same icon, same
// curves and same times in both, so a notice reads the same wherever it shows.
//   const notice = bhNotice(host, send)   → host: the element that becomes the notice
//   notice.show({ text, icon, tone, stay, actions: [{ label, run }] })
//   notice.visible(false)                 → another host shows it: this one steps back, and
//                                           takes over where the time is if it comes back
//   notice.done()                         → answered or replaced elsewhere
// The page places and sizes its host (`.show` while on screen); texts by textContent only.
(() => {
  if (window.bhNotice) return;
  const css = `
  .bh-notice { --nt: var(--bh-accent, #7c5cff); display: flex; align-items: center; gap: var(--bh-space-8, 8px); overflow: hidden; }
  .bh-notice.good { --nt: var(--bh-success, #4ade80); }
  .bh-notice.bad { --nt: var(--bh-danger-text, #ff8a8a); }
  .bh-notice .nic { flex: none; display: grid; place-items: center; color: var(--nt); }
  .bh-notice.show .nic { animation: bh-notice-icon var(--bh-duration-spring, .3s) var(--bh-ease-pop, cubic-bezier(.2,1.3,.4,1)) backwards; }
  .bh-notice .txt { flex: 1; min-width: 0; }
  .bh-notice button { flex: none; width: auto; height: auto; border: 0; border-radius: 7px; padding: 3px 9px; background: var(--bh-accent, #7c5cff); color: var(--bh-text-on-accent, #fff); font: inherit; font-weight: 600; line-height: 1.4; cursor: pointer; opacity: 1; }
  .bh-notice button:hover { background: color-mix(in srgb, var(--bh-accent, #7c5cff) 82%, white); opacity: 1; }
  .bh-notice button.alt { background: var(--bh-fill-control, rgba(255,255,255,.12)); color: inherit; font-weight: 500; }
  .bh-notice button.alt:hover { background: var(--bh-fill-control-hover, rgba(255,255,255,.2)); }
  .bh-notice .tbar { position: absolute; left: 0; bottom: 0; width: 100%; height: 2px; background: var(--nt); transform-origin: 0 0; transform: scaleX(0); }
  .bh-notice.show .tbar { animation: toast-time var(--t, 2200ms) linear both; }
  @keyframes toast-time { from { transform: scaleX(1); } to { transform: scaleX(0); } }
  @keyframes bh-notice-icon { from { opacity: 0; transform: scale(.4) rotate(-30deg); } }
  @media (prefers-reduced-motion: reduce) { .bh-notice, .bh-notice * { animation: none !important; transition: none !important; } }
  `;
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(css);
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  } catch (e) {
    const style = document.createElement('style');
    style.textContent = css;
    document.documentElement.append(style);
  }
  // Too little time left to read anything: a host that takes over then shows nothing.
  const WORTH = 400;
  const el = (tag, cls, text) => {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text;
    return n;
  };
  window.bhNotice = (host, send) => {
    host.classList.add('bh-notice');
    let cur = null, timer = 0, on = true;
    const hide = () => { clearTimeout(timer); host.classList.remove('show'); };
    const draw = () => {
      clearTimeout(timer);
      const left = cur ? cur.until - Date.now() : 0;
      if (!on || left < WORTH) return hide();
      const icon = el('span', 'nic');
      try { icon.append(bhIcon(cur.icon || 'info', 'sm')); } catch (e) {}
      const buttons = (cur.actions || []).map((a, i) => {
        const b = el('button', i ? 'alt' : null, a.label);
        b.onclick = () => {
          cur = null;
          hide();
          // Before the answer: what it does may show a notice of its own.
          send({ cmd: 'notice_done' });
          send(a.run);
        };
        return b;
      });
      const bar = el('i', 'tbar');
      // Taken over from another host: the line goes on from where the time is.
      if (cur.stay - left > 60) bar.style.animationDelay = (left - cur.stay) + 'ms';
      host.replaceChildren(icon, el('span', 'txt', cur.text), ...buttons, bar);
      host.title = cur.text;
      host.style.setProperty('--t', cur.stay + 'ms');
      host.classList.toggle('act', buttons.length > 0);
      host.classList.toggle('good', cur.tone === 'good');
      host.classList.toggle('bad', cur.tone === 'bad');
      host.classList.add('show');
      timer = setTimeout(hide, left);
    };
    return {
      show(n) {
        const stay = n.stay || (n.actions && n.actions.length ? 8000 : 2200);
        cur = { ...n, stay, until: Date.now() + stay };
        draw();
      },
      done() { cur = null; hide(); },
      visible(v) {
        if (v === on) return;
        on = v;
        draw();
      },
      shown: () => host.classList.contains('show'),
    };
  };
})();
