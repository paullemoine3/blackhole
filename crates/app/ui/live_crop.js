// Injected in the live view of an Easel card (top frame only): scales the page so that the
// captured element exactly fills the view, and hides everything else.
(() => {
  if (window.top !== window) return;
  const SELECTOR = __SELECTOR__;
  let done = 0;
  const fit = () => {
    let el = null;
    try {
      el = document.querySelector(SELECTOR);
    } catch (_) {}
    const root = document.documentElement;
    if (!el || !document.body) return;
    root.style.setProperty('overflow', 'hidden', 'important');
    document.body.style.setProperty('overflow', 'hidden', 'important');
    document.body.style.setProperty('transform', 'none', 'important');
    window.scrollTo(0, 0);
    const r = el.getBoundingClientRect();
    if (!r.width || !r.height) return;
    const s = Math.min(innerWidth / r.width, innerHeight / r.height);
    const b = document.body;
    // The transform turns around the body's own corner, which is not the page's when the body
    // is centred or has a margin: the element is brought to that corner, scaled, and the corner
    // to the view's.
    const o = b.getBoundingClientRect();
    b.style.setProperty('transform-origin', '0 0', 'important');
    b.style.setProperty('transform', `translate(${-o.left}px, ${-o.top}px) scale(${s}) translate(${-(r.left - o.left)}px, ${-(r.top - o.top)}px)`, 'important');
    window.__bhLiveFit = { s, left: r.left, top: r.top };
  };
  const again = () => {
    fit();
    if (done++ < 4) setTimeout(again, [500, 1500, 3000, 6000][done - 1]);
  };
  addEventListener('DOMContentLoaded', again);
  addEventListener('load', fit);
  addEventListener('resize', () => {
    done = 0;
    fit();
  });
})();
