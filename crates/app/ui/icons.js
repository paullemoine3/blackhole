// BlackHole's icons, injected in each of its own pages with theme.js: one stroke set drawn the
// same on every system (emoji and font glyphs are not). A 16-unit grid, `currentColor`, round
// caps and a 1.5 px stroke that keeps its width at every size (theme.js: `.bh-i`).
// Sizes follow the scale of the design system: xs 12 (inline marks), sm 16 (default: menus,
// buttons, bars, rows), md 20 (the lead icon of a card), lg 24 (empty states, bare tiles).
//   bhIcon('gear')          → an <svg> element (16 px)
//   bhIcon('gear', 'md')    → 20 px
//   bhIconHTML('gear')      → the same as markup, for pages that build their HTML as text
//   <i data-icon="gear" data-size="xs"></i> in a page's markup is replaced when it loads.
// The markup only ever comes from this table: never pass page or site data to these functions.
(() => {
  if (window.bhIcon) return;
  const dot = (x, y) => `<circle cx="${x}" cy="${y}" r="1.15" fill="currentColor" stroke="none"/>`;
  const P = {
    // Navigation and window
    back: '<path d="M13 8H3.5M7.5 4L3.5 8l4 4"/>',
    forward: '<path d="M3 8h9.5M8.5 4l4 4-4 4"/>',
    reload: '<path d="M12.5 8A4.5 4.5 0 1 1 11.1 4.7M12.5 2.5v2.8H9.7"/>',
    home: '<path d="M2.5 7.3L8 2.8l5.5 4.5M4 6.2v6.3c0 .6.4 1 1 1h2v-3.5h2v3.5h2c.6 0 1-.4 1-1V6.2"/>',
    sidebar: '<rect x="2" y="3" width="12" height="10" rx="2"/><path d="M6 3v10"/>',
    window: '<rect x="2" y="3" width="12" height="10" rx="2"/><path d="M2 6h12"/>',
    split: '<rect x="2" y="3" width="12" height="10" rx="2"/><path d="M8 3v10"/>',
    pip: '<rect x="1.5" y="3" width="13" height="10" rx="1.8"/><rect x="8" y="7.8" width="4.5" height="3.2" rx=".7"/>',
    expand: '<path d="M4 6l4 4 4-4"/>',
    collapse: '<path d="M6 4l4 4-4 4"/>',
    out: '<path d="M8 13V3.5M4.5 7L8 3.5 11.5 7"/>',
    open: '<path d="M6 3.5H3.5v9h9V10M9 3h4v4M13 3L7.5 8.5"/>',
    go: '<path d="M5 11l6-6M6 5h5v5"/>',
    switch: '<path d="M3 8h9M8.5 4.5L12 8l-3.5 3.5"/>',
    // Actions
    plus: '<path d="M8 3v10M3 8h10"/>',
    minus: '<path d="M3.5 8h9"/>',
    close: '<path d="M4 4l8 8M12 4l-8 8"/>',
    check: '<path d="M3.5 8.5l3 3 6-7"/>',
    more: dot(3.5, 8) + dot(8, 8) + dot(12.5, 8),
    edit: '<path d="M10.5 3l2.5 2.5L6 12.5H3.5V10z"/>',
    marker: '<path d="M9.5 2.5l3 3-5 5H4.5v-3z"/><path d="M2.5 13.5h11"/>',
    trash: '<path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.7 9h5.6l.7-9"/>',
    copy: '<rect x="5.5" y="5.5" width="8" height="8" rx="1.5"/><path d="M10.5 5.5V3.5c0-.6-.4-1-1-1h-6c-.6 0-1 .4-1 1v6c0 .6.4 1 1 1h2"/>',
    link: '<path d="M7 9a2.8 2.8 0 0 0 4 0l2-2a2.8 2.8 0 0 0-4-4l-.7.7M9 7a2.8 2.8 0 0 0-4 0l-2 2a2.8 2.8 0 0 0 4 4l.7-.7"/>',
    pin: '<path d="M6 2.5h4M7 2.5v4L4.5 9h7L9 6.5v-4M8 9v4.5"/>',
    sleep: '<path d="M12.5 9.5A5 5 0 1 1 6.5 3.5a4 4 0 0 0 6 6z"/>',
    hole: '<circle cx="8" cy="8" r="2.3" fill="currentColor" stroke="none"/><ellipse cx="8" cy="8" rx="6.3" ry="2.7" transform="rotate(-22 8 8)"/>',
    search: '<circle cx="7" cy="7" r="4.5"/><path d="M10.3 10.3L13.5 13.5"/>',
    bolt: '<path d="M9 1.8L3.5 9h4l-1 5.2L12.5 7h-4z"/>',
    download: '<path d="M8 2v8M4.5 6.5L8 10l3.5-3.5M2.5 13.5h11"/>',
    tidy: '<path d="M2.5 4.5h11M2.5 8h7M2.5 11.5h4M12 9.5l1.5 1.5 2-3"/>',
    spinner: '<path d="M8 2.5a5.5 5.5 0 1 1-5.5 5.5"/>',
    alert: '<path d="M8 2.5l6 10.5H2zM8 6.8v3"/>' + dot(8, 11.6),
    // Media
    play: '<path d="M5 3.3v9.4L12.4 8z" fill="currentColor"/>',
    pause: '<path d="M5.5 3.5v9M10.5 3.5v9"/>',
    sound: '<path d="M2.5 6v4h2.5l3.5 3V3L5 6zM11 5.5a3.5 3.5 0 0 1 0 5"/>',
    mute: '<path d="M2.5 6v4h2.5l3.5 3V3L5 6zM11 6l3 4M14 6l-3 4"/>',
    mic: '<rect x="6" y="2" width="4" height="7.5" rx="2"/><path d="M3.5 7.8a4.5 4.5 0 0 0 9 0M8 12.3v1.7"/>',
    'mic-off': '<path d="M10 7V4a2 2 0 0 0-3.8-.9M6 6.5v1a2 2 0 0 0 3 1.7M3.5 7.8a4.5 4.5 0 0 0 7.2 3.6M12.5 7.8c0 .5-.1 1-.2 1.4M8 12.3v1.7M2.5 2.5l11 11"/>',
    video: '<rect x="1.5" y="4" width="9" height="8" rx="1.8"/><path d="M10.5 7l4-2.2v6.4l-4-2.2"/>',
    'video-off': '<path d="M5 4h3.7c1 0 1.8.8 1.8 1.8V9M10.5 7l4-2.2v6.4l-4-2.2M3.3 4.1A1.8 1.8 0 0 0 1.5 5.8v4.4c0 1 .8 1.8 1.8 1.8h5.4c.5 0 .9-.2 1.2-.5M2 2.5l11 11"/>',
    camera: '<path d="M2 5.5c0-.6.4-1 1-1h2l1-1.5h4l1 1.5h2c.6 0 1 .4 1 1V12c0 .6-.4 1-1 1H3c-.6 0-1-.4-1-1z"/><circle cx="8" cy="8.5" r="2.3"/>',
    print: '<path d="M4.5 6V2.5h7V6M4.5 11.5h-2V6h11v5.5h-2"/><path d="M4.5 9.5h7v4h-7z"/>',
    image: '<rect x="2" y="2.5" width="12" height="11" rx="2"/><circle cx="6" cy="6.3" r="1.3"/><path d="M2.5 12l3.5-3.5 2.5 2.5 2-2 3 3"/>',
    paint: '<path d="M13.5 2.5L7.5 8.5M6.2 9c-1.6 0-2.7 1-2.7 2.5 0 .9-.4 1.4-1 1.7 1.1.6 4.6.7 5.2-1.5.3-1.1-.3-2.4-1.5-2.7z"/>',
    // Places and things
    gear: '<circle cx="8" cy="8" r="2.3"/><path d="M8 1.8v1.7M8 12.5v1.7M1.8 8h1.7M12.5 8h1.7M3.6 3.6l1.2 1.2M11.2 11.2l1.2 1.2M3.6 12.4l1.2-1.2M11.2 4.8l1.2-1.2"/>',
    shield: '<path d="M8 1.8l5 2v3.7c0 3.2-2.1 5.6-5 6.7-2.9-1.1-5-3.5-5-6.7V3.8z"/>',
    lock: '<rect x="3" y="7" width="10" height="7" rx="1.6"/><path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2M8 10v1.2"/>',
    folder: '<path d="M2 4.5c0-.6.4-1 1-1h3l1.5 1.5H13c.6 0 1 .4 1 1V12c0 .6-.4 1-1 1H3c-.6 0-1-.4-1-1z"/>',
    library: '<path d="M3 2.5v11M6.2 2.5v11M9.2 3.3l3.3 10"/>',
    archive: '<rect x="2" y="2.8" width="12" height="3.4" rx="1"/><path d="M3 6.2V12c0 .6.4 1 1 1h8c.6 0 1-.4 1-1V6.2M6.5 9h3"/>',
    history: '<path d="M2.8 8a5.2 5.2 0 1 0 1.6-3.8M2.5 2.4v2.7h2.7M8 5v3l2 1.4"/>',
    grid: '<rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1.1"/><rect x="9" y="2.5" width="4.5" height="4.5" rx="1.1"/><rect x="2.5" y="9" width="4.5" height="4.5" rx="1.1"/><rect x="9" y="9" width="4.5" height="4.5" rx="1.1"/>',
    easel: '<rect x="2.5" y="2.5" width="11" height="8" rx="1.5"/><path d="M5 13.5l1.5-3M11 13.5l-1.5-3"/>',
    space: '<circle cx="8" cy="8" r="5.5"/><circle cx="8" cy="8" r="2"/>',
    emoji: '<circle cx="8" cy="8" r="5.5"/><path d="M5.8 9.6c.6.8 1.3 1.1 2.2 1.1s1.6-.3 2.2-1.1M6 6.5v.1M10 6.5v.1"/>',
    eye: '<path d="M1.5 8s2.4-4.5 6.5-4.5S14.5 8 14.5 8 12.1 12.5 8 12.5 1.5 8 1.5 8z"/><circle cx="8" cy="8" r="2"/>',
    watch: '<circle cx="8" cy="8" r="5.5"/><path d="M8 5v3l2 1.5"/>',
    compare: '<rect x="2" y="3" width="5" height="10" rx="1"/><rect x="9" y="3" width="5" height="10" rx="1"/>',
    devices: '<rect x="1.5" y="3" width="9" height="7" rx="1.2"/><path d="M4 12.5h4"/><rect x="11" y="5.5" width="3.5" height="7.5" rx="1"/>',
    code: '<path d="M5.5 4.5L2 8l3.5 3.5M10.5 4.5L14 8l-3.5 3.5"/>',
    keyboard: '<rect x="1.5" y="4" width="13" height="8.5" rx="1.8"/><path d="M4 7h.1M6.5 7h.1M9 7h.1M11.5 7h.1M5 9.8h6"/>',
    puzzle: '<path d="M6 2.5h2.5v2a1.3 1.3 0 0 0 2.6 0v-2h2.4V6h-2a1.3 1.3 0 0 0 0 2.6h2v4.9H9.6v-2a1.3 1.3 0 0 0-2.6 0v2H2.5V9h2a1.3 1.3 0 0 0 0-2.6h-2V2.5z"/>',
    calendar: '<rect x="2" y="3" width="12" height="11" rx="2"/><path d="M2 6.5h12M5.5 1.8v2.4M10.5 1.8v2.4"/>',
    key: '<circle cx="5.5" cy="10.5" r="3"/><path d="M7.6 8.4L13.5 2.5M11 5l1.8 1.8M9.5 6.5l1.3 1.3"/>',
    import: '<path d="M8 2v8M4.5 6.5L8 10l3.5-3.5M2.5 13.5h11"/>',
    user: '<circle cx="8" cy="5.5" r="2.8"/><path d="M2.8 13.5c.6-2.5 2.7-4 5.2-4s4.6 1.5 5.2 4"/>',
    globe: '<circle cx="8" cy="8" r="5.8"/><path d="M2.2 8h11.6M8 2.2c1.6 1.6 2.4 3.5 2.4 5.8S9.6 12.2 8 13.8C6.4 12.2 5.6 10.3 5.6 8S6.4 3.8 8 2.2z"/>',
    info: '<circle cx="8" cy="8" r="5.8"/><path d="M8 7.3v3.6"/>' + dot(8, 5.2),
    sparkle: '<path d="M8 2l1.4 4.6L14 8l-4.6 1.4L8 14l-1.4-4.6L2 8l4.6-1.4z"/>',
    rocket: '<path d="M8 1.5c2.2 1.7 3.2 4 3.2 6.8v3.2H4.8V8.3C4.8 5.5 5.8 3.2 8 1.5z"/><circle cx="8" cy="6.6" r="1.3"/><path d="M4.8 9.2l-2.3 2.3v2h2.3M11.2 9.2l2.3 2.3v2h-2.3M7 13.2v1.3M9 13.2v1.3"/>',
  };
  P.settings = P.gear;
  const SIZES = { xs: 1, sm: 1, md: 1, lg: 1 };
  const html = (name, size, cls) => {
    const s = SIZES[size] ? size : 'sm';
    return '<svg class="bh-i bh-i-' + s + (cls ? ' ' + cls : '') + '" viewBox="0 0 16 16" aria-hidden="true" focusable="false" data-i="' + (P[name] ? name : '') + '">' + (P[name] || '') + '</svg>';
  };
  const make = (name, size, cls) => {
    const t = document.createElement('template');
    t.innerHTML = html(name, size, cls);
    return t.content.firstChild;
  };
  const hydrate = (root) => {
    (root || document).querySelectorAll('i[data-icon]').forEach((i) => {
      const svg = make(i.dataset.icon, i.dataset.size, i.className);
      i.replaceWith(svg);
    });
  };
  Object.defineProperty(window, 'bhIcon', { value: make });
  Object.defineProperty(window, 'bhIconHTML', { value: html });
  Object.defineProperty(window, 'bhIcons', { value: Object.freeze({ names: Object.keys(P), has: (n) => !!P[n], hydrate }) });
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', () => hydrate());
  else hydrate();
})();
