// Injected in every BlackHole view: native web views swallow key events, so app shortcuts
// are forwarded to the shell from inside the page.
(() => {
  if (!window.__bhipc || window.top !== window) return;
  const mac = navigator.platform.toLowerCase().includes('mac');
  const KEYS = ['l', 't', 'w', 'r', 'b', '[', ']', ',', '\\', 'enter', 'n', 'f', 'g', 'p'];
  // Zoom: "+" is Mod+= on most keyboards, with or without Shift.
  const ZOOM = { '=': 'zoom_in', '+': 'zoom_in', '-': 'zoom_out', _: 'zoom_out', 0: 'zoom_reset' };
  // Find keys go to the page first: apps with their own search (Google Docs draws its text on a
  // canvas, which the find bar cannot read) take them with preventDefault; otherwise the bar opens.
  const PAGE_FIRST = new Set(['f', 'g', 'shift+g']);
  const post = (key) => window.__bhipc.postMessage(JSON.stringify({ cmd: 'shortcut', key }));

  // The shortcut a key press stands for, or null.
  const shortcutOf = (e) => {
    // Ctrl+Tab (Ctrl on every system, as in other browsers) goes back to the previous tab.
    if (e.key === 'Tab' && e.ctrlKey && !e.metaKey && !e.altKey) return 'recent_tab';
    // Mod+Alt+↓/↑ walks the sidebar, like Arc.
    if ((mac ? e.metaKey : e.ctrlKey) && e.altKey && !e.shiftKey && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
      return e.key === 'ArrowDown' ? 'next_tab' : 'prev_tab';
    }
    // Windows: the engine's own browser keys are off in tabs (see views::build_tab), so
    // F5 / Ctrl+F5 and Alt+←/→ are handled here like any shortcut.
    if (!mac && e.key === 'F5' && !e.altKey && !e.shiftKey) return e.ctrlKey ? 'r' : 'reload';
    if (!mac && e.altKey && !e.ctrlKey && !e.shiftKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
      return e.key === 'ArrowLeft' ? '[' : ']';
    }
    if ((mac ? !e.metaKey : !e.ctrlKey) || e.altKey) return null;
    // AZERTY layouts type digits with Shift: the physical key tells it is a zero.
    const key = e.code === 'Digit0' || e.code === 'Numpad0' ? '0' : e.key.toLowerCase();
    if (ZOOM[key]) return ZOOM[key];
    // Shift: Mod+Shift+N is a private window, Mod+Shift+E opens Easel, Mod+Shift+S clips the page,
    // Mod+Shift+L picks an area to capture live, Mod+Shift+G finds the previous match,
    // Mod+Shift+T reopens the tab closed last.
    if (e.shiftKey) return ['n', 'e', 's', 'l', 'g', 't'].includes(key) ? 'shift+' + key : null;
    if (!KEYS.includes(key) || (key === 'n' && window.keepCtrlN)) return null;
    return key;
  };

  addEventListener(
    'keydown',
    (e) => {
      // Escape is only forwarded (never swallowed): it closes the palette or a peek.
      if (e.key === 'Escape') return post('escape');
      const key = shortcutOf(e);
      if (!key || PAGE_FIRST.has(key)) return;
      e.preventDefault();
      e.stopPropagation();
      post(key);
    },
    true,
  );
  addEventListener('keydown', (e) => {
    const key = shortcutOf(e);
    if (!key || !PAGE_FIRST.has(key) || e.defaultPrevented) return;
    e.preventDefault();
    post(key);
  });
})();
