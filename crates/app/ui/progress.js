// The loading line at the top of the page itself (0.8.25), in place of the one under the top
// bar: the page card is a native view, nothing of the shell can be drawn over it. Drawn by a
// generated box (`html::before`) from a constructed sheet: no node is added to the page (a page
// that rebuilds its document, React hydration included, never sees it), and page CSPs do not
// block it. The new document starts it; the shell gives it the space's color when the engine
// says the page started (`tint`) and ends it when the page loaded or failed (`done`). Before the
// new document exists (a server slow to answer), the top bar draws the line (bar.html).
(() => {
  if (window.top !== window || window.__bhProgress) return;
  const BH = '#7c5cff';
  let sheet = null, color = BH, calm = false, phase = '', end = 0;
  const css = (rule) => {
    try {
      if (!sheet) {
        sheet = new CSSStyleSheet();
        document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
      }
      sheet.replaceSync(rule);
    } catch (e) {}
  };
  const drop = () => {
    if (!sheet) return;
    try { document.adoptedStyleSheets = document.adoptedStyleSheets.filter((s) => s !== sheet); } catch (e) {}
    sheet = null;
  };
  const line = (more) =>
    `html::before{content:"";position:fixed;z-index:2147483647;top:0;left:0;height:2px;pointer-events:none;` +
    `border-radius:0 2px 2px 0;background:linear-gradient(90deg,${color},#ffb020);box-shadow:0 0 6px ${color};${more}}`;
  const still = '@media (prefers-reduced-motion: reduce){html::before{animation:none!important;transition:none!important}}';
  const run = (from) => {
    phase = 'run';
    clearTimeout(end);
    const moving = calm ? 'width:85%' : 'width:85%;animation:bh-progress 12s cubic-bezier(.1,.7,.2,1) both';
    css(`@keyframes bh-progress{from{width:${from}%}to{width:85%}}` + line(moving) + still);
  };
  const done = () => {
    if (phase !== 'run') return;
    phase = 'done';
    css(line('width:100%;opacity:0;transition:width .2s ease-out,opacity .3s ease .2s') + still);
    end = setTimeout(drop, calm ? 0 : 600);
  };
  const safeColor = (c) => (typeof c === 'string' && /^#[0-9a-f]{6}$/i.test(c) ? c : BH);
  Object.defineProperty(window, '__bhProgress', {
    value: Object.freeze({
      // The engine says this document started: the space's color, the animations setting.
      tint(c, animations) {
        color = safeColor(c);
        calm = animations === false;
        if (phase === 'run') run(30);
      },
      done,
    }),
  });
  if (document.readyState !== 'complete') {
    run(30);
    addEventListener('load', () => setTimeout(done, 0), { once: true });
  }
})();
