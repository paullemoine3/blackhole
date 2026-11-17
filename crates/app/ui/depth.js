// The third dimension of BlackHole's own pages, injected in each of them with theme.js: a card
// rises under the pointer. It no longer turns to follow the pointer (0.8.16: it did not look
// elegant); only its elevation is left. A page names what rises:
//   bhDepth('.fav', 12)   → the element under the pointer that matches gets the class `bh-lean`,
//                            and loses it when the pointer leaves it. The degrees are kept for
//                            the API but unused. The four variables the pages read stay neutral:
//                            --rx, --ry  0 (no turn)
//                            --mx, --my  50% (a glint stays in the middle of the card)
// The page writes the transform itself (it owns the hover and press states of its cards):
//   .fav.bh-lean { transform: perspective(600px) … translateZ(3px); }   → the card comes closer
// Nothing rises in the flat style of the interface (`body.flat`, Settings > Appearance), while
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
  const VARS = { '--rx': '0', '--ry': '0', '--mx': '50%', '--my': '50%' };
  let on = null;
  const still = () => {
    const b = document.body;
    return !b || b.classList.contains('flat') || b.classList.contains('noanim') || b.classList.contains('still') || b.classList.contains('dragging')
      || matchMedia('(prefers-reduced-motion: reduce)').matches;
  };
  const rest = () => {
    if (!on) return;
    on.classList.remove('bh-lean');
    for (const v in VARS) on.style.removeProperty(v);
    on = null;
  };
  const move = (e) => {
    let hit = null;
    if (e.pointerType !== 'touch' && e.target && e.target.closest && !still()) {
      for (const [selector] of rules) {
        const n = e.target.closest(selector);
        // The innermost match wins: a tile inside a card that rises too.
        if (n && (!hit || hit.contains(n))) hit = n;
      }
    }
    if (on === hit) return;
    rest();
    if (!hit) return;
    on = hit;
    for (const v in VARS) hit.style.setProperty(v, VARS[v]);
    hit.classList.add('bh-lean');
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
