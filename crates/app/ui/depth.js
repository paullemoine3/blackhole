// The third dimension of BlackHole's own pages, injected in each of them with theme.js: a card
// leans towards the pointer, as on the project's web page. A page names what leans:
//   bhDepth('.fav', 12)   → the element under the pointer that matches gets the class `bh-lean`
//                            and four variables, and loses them when the pointer leaves it:
//                            --rx, --ry  degrees to turn around the x and y axes (at most 12)
//                            --mx, --my  where the pointer is on it, in percent (for a glint)
// The page writes the transform itself (it owns the hover and press states of its cards):
//   .fav.bh-lean { transform: perspective(600px) rotateX(calc(var(--rx) * 1deg)) rotateY(calc(var(--ry) * 1deg)); }
// Nothing leans in the flat style of the interface (`body.flat`, Settings > Appearance), while
// animations are off (`body.noanim`, `body.still`, the system's reduced motion), during a drag
// (`body.dragging`), or under a finger.
(() => {
  if (window.bhDepth) return;
  const rules = [];
  const VARS = ['--rx', '--ry', '--mx', '--my'];
  let on = null;
  const still = () => {
    const b = document.body;
    return !b || b.classList.contains('flat') || b.classList.contains('noanim') || b.classList.contains('still') || b.classList.contains('dragging')
      || matchMedia('(prefers-reduced-motion: reduce)').matches;
  };
  const rest = () => {
    if (!on) return;
    on.classList.remove('bh-lean');
    for (const v of VARS) on.style.removeProperty(v);
    on = null;
  };
  const move = (e) => {
    let hit = null, max = 0;
    if (e.pointerType !== 'touch' && e.target && e.target.closest && !still()) {
      for (const [selector, degrees] of rules) {
        const n = e.target.closest(selector);
        // The innermost match wins: a tile inside a card that leans too.
        if (n && (!hit || hit.contains(n))) { hit = n; max = degrees; }
      }
    }
    if (on !== hit) rest();
    if (!hit) return;
    const r = hit.getBoundingClientRect();
    if (!r.width || !r.height) return;
    const x = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), y = Math.min(1, Math.max(0, (e.clientY - r.top) / r.height));
    on = hit;
    hit.classList.add('bh-lean');
    hit.style.setProperty('--ry', ((x - .5) * 2 * max).toFixed(2));
    hit.style.setProperty('--rx', ((.5 - y) * 2 * max).toFixed(2));
    hit.style.setProperty('--mx', (x * 100).toFixed(1) + '%');
    hit.style.setProperty('--my', (y * 100).toFixed(1) + '%');
  };
  document.addEventListener('pointermove', move, { passive: true });
  document.addEventListener('pointerdown', rest, { passive: true });
  document.addEventListener('scroll', rest, { passive: true, capture: true });
  // The pointer left the page. On `document`: this script runs before the page has a root element.
  document.addEventListener('mouseout', (e) => { if (!e.relatedTarget) rest(); }, { passive: true });
  addEventListener('blur', rest);
  Object.defineProperty(window, 'bhDepth', {
    value: (selector, degrees) => { rules.push([String(selector), Math.min(16, Math.abs(+degrees || 8))]); },
  });
})();
