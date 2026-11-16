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
//   --bh-relief-row      a strip barely off its panel: two fine edges, no cast shadow (a quick
//                        access tile, the tab shown, New tab). Tabs and quick access stay quiet:
//                        no sheen, no key, a tab at rest has no strip at all.
//   --bh-relief-key      a coloured tile as thick as a key (`--bh-relief-face` + its edge and shadow)
//   --bh-relief-key-down the same key held down
//   --bh-relief-sunk     a well cut in the surface (a field, a track)
//   --bh-sheen           the light on a coloured face (a background for its `::after`)
//   --bh-sheen-soft      the same light on a translucent surface (a `background-image`, which
//                        leaves the surface its own `box-shadow`: a border in a state's colour)
// The relief is low on purpose (edges of one pixel, short soft shadows, a faint light): it
// must read as depth, never as decoration. It is no motion: it stays with animations off.
(() => {
  if (window.bhDepth) return;
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(`:where(:root) {
      --bh-relief-face: inset 0 1px 0 #ffffff30, inset 0 -1px 0 #00000026;
      --bh-relief-key: var(--bh-relief-face), 0 1px 0 #00000052, 0 6px 12px -8px #00000099;
      --bh-relief-key-down: var(--bh-relief-face), 0 1px 2px #00000073;
      --bh-relief-raised: inset 0 1px 0 #ffffff14, inset 0 -1px 0 #0000002e, 0 1px 0 #00000038, 0 8px 14px -12px #00000099;
      --bh-relief-row: inset 0 1px 0 #ffffff12, inset 0 -1px 0 #00000024, 0 1px 0 #00000033;
      --bh-relief-panel: inset 0 1px 0 #ffffff12, inset 0 -1px 0 #00000038, 0 16px 30px -22px #000000a6;
      --bh-relief-sunk: inset 0 1px 3px #00000061, inset 0 -1px 0 #ffffff0f;
      --bh-sheen: linear-gradient(180deg, #ffffff1c, #ffffff00 50%, #00000012);
      --bh-sheen-soft: linear-gradient(180deg, #ffffff0b, #ffffff00 45%, #0000000f);
    }`);
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  } catch (e) {}
  const rules = [];
  const VARS = ['--rx', '--ry', '--mx', '--my'];
  let on = null, onMax = 0, base = null;
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
    base = null;
  };
  // Where the card was before it leaned: a turned card moves its own edges, and judging by
  // them made a card at the pointer's edge lean, lose the pointer, fall flat, find it again…
  // forever. The card keeps the pointer while it is inside this rectangle.
  const inside = (r, e) => r && e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom;
  const move = (e) => {
    let hit = null, max = 0;
    if (e.pointerType !== 'touch' && e.target && e.target.closest && !still()) {
      for (const [selector, degrees] of rules) {
        const n = e.target.closest(selector);
        // The innermost match wins: a tile inside a card that leans too.
        if (n && (!hit || hit.contains(n))) { hit = n; max = degrees; }
      }
    }
    if (on && hit !== on && (!hit || hit.contains(on)) && inside(base, e) && !still() && e.pointerType !== 'touch') {
      hit = on; max = onMax;
    }
    if (on !== hit) rest();
    if (!hit) return;
    if (!base) {
      const r0 = hit.getBoundingClientRect();
      if (!r0.width || !r0.height) return;
      base = { left: r0.left, top: r0.top, right: r0.right, bottom: r0.bottom, width: r0.width, height: r0.height };
    }
    const r = base;
    const x = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)), y = Math.min(1, Math.max(0, (e.clientY - r.top) / r.height));
    // Calm near the edges, where the buttons are (close, menu): the card is flat under them.
    const edge = Math.min(x, 1 - x, y, 1 - y), calm = Math.max(0, Math.min(1, (edge - .04) / .14));
    on = hit;
    onMax = max;
    hit.classList.add('bh-lean');
    hit.style.setProperty('--ry', ((x - .5) * 2 * max * calm).toFixed(2));
    hit.style.setProperty('--rx', ((.5 - y) * 2 * max * calm).toFixed(2));
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
