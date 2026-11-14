// The third dimension of BlackHole's own pages, injected in each of them with theme.js: a card
// leans towards the pointer, as on the project's web page. A page names what leans:
//   bhDepth('.fav', 12)   → the element under the pointer that matches gets the class `bh-lean`
//                            and four variables, and loses them when the pointer leaves it:
//                            --rx, --ry  degrees to turn around the x and y axes (at most 12 here, 24 allowed)
//                            --mx, --my  where the pointer is on it, in percent (for a glint)
// The page writes the transform itself (it owns the hover and press states of its cards):
//   .fav.bh-lean { transform: perspective(600px) rotateX(calc(var(--rx) * 1deg)) rotateY(calc(var(--ry) * 1deg)); }
// Nothing leans in the flat style of the interface (`body.flat`, Settings > Appearance), while
// animations are off (`body.noanim`, `body.still`, the system's reduced motion), during a drag
// (`body.dragging`), or under a finger.
//
// The 3D style is also there at rest: surfaces have a relief, lit from above. This script gives
// the shadows as variables, a page puts them on its surfaces under `body:not(.flat)`:
//   --bh-relief-panel    a large surface standing off the page (a section, a settings card)
//   --bh-relief-raised   a translucent control or row standing off its panel
//   --bh-relief-key      a coloured tile as thick as a key (`--bh-relief-face` + its edge and shadow)
//   --bh-relief-key-down the same key held down
//   --bh-relief-sunk     a well cut in the surface (a field, a track)
//   --bh-sheen           the light on a coloured face (a background for its `::after`)
//   --bh-sheen-soft      the same light on a translucent surface (a `background-image`, which
//                        leaves the surface its own `box-shadow`: a border in a state's colour)
// The relief is no motion: it stays with animations off.
(() => {
  if (window.bhDepth) return;
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(`:where(:root) {
      --bh-relief-face: inset 0 1px 0 #ffffff5c, inset 0 -3px 0 #0000003d;
      --bh-relief-key: var(--bh-relief-face), 0 3px 0 #00000080, 0 12px 18px -8px #000000d9;
      --bh-relief-key-down: var(--bh-relief-face), 0 1px 0 #00000080, 0 3px 6px -2px #000000b3;
      --bh-relief-raised: inset 0 1px 0 #ffffff24, inset 0 -1px 0 #00000052, 0 2px 0 #00000059, 0 10px 18px -10px #000000cc;
      --bh-relief-panel: inset 0 1px 0 #ffffff1f, inset 0 -1px 0 #00000066, 0 24px 48px -20px #000000e6, 0 3px 8px #00000066;
      --bh-relief-sunk: inset 0 2px 6px #0000008c, inset 0 -1px 0 #ffffff1a;
      --bh-sheen: linear-gradient(180deg, #ffffff38, #ffffff00 48%, #00000033);
      --bh-sheen-soft: linear-gradient(180deg, #ffffff17, #ffffff00 45%, #00000024);
    }`);
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  } catch (e) {}
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
    value: (selector, degrees) => { rules.push([String(selector), Math.min(24, Math.abs(+degrees || 8))]); },
  });
})();
