// Injected in every BlackHole view: native web views swallow key events, so app shortcuts
// are forwarded to the shell from inside the page.
(() => {
  if (!window.__bhipc || window.top !== window) return;
  const mac = navigator.platform.toLowerCase().includes('mac');
  const KEYS = ['l', 't', 'w', 'r', 'b', '[', ']', ',', '\\', 'enter', 'n', 'f', 'g', 'p'];
  // Zoom: "+" is Mod+= on most keyboards, with or without Shift.
  const ZOOM = { '=': 'zoom_in', '+': 'zoom_in', '-': 'zoom_out', _: 'zoom_out', 0: 'zoom_reset' };
  addEventListener(
    'keydown',
    (e) => {
      // Escape is only forwarded (never swallowed): it closes the palette or a peek.
      if (e.key === 'Escape') return window.__bhipc.postMessage(JSON.stringify({ cmd: 'shortcut', key: 'escape' }));
      if ((mac ? !e.metaKey : !e.ctrlKey) || e.altKey) return;
      // AZERTY layouts type digits with Shift: the physical key tells it is a zero.
      let key = e.code === 'Digit0' || e.code === 'Numpad0' ? '0' : e.key.toLowerCase();
      // Shift: Mod+Shift+N is a private window, Mod+Shift+E opens Easel, Mod+Shift+S clips the page,
      // Mod+Shift+L picks an area to capture live, Mod+Shift+G finds the previous match.
      if (ZOOM[key]) key = ZOOM[key];
      else if (e.shiftKey) {
        if (!['n', 'e', 's', 'l', 'g'].includes(key)) return;
        key = 'shift+' + key;
      } else if (!KEYS.includes(key) || (key === 'n' && window.keepCtrlN)) return;
      e.preventDefault();
      e.stopPropagation();
      window.__bhipc.postMessage(JSON.stringify({ cmd: 'shortcut', key }));
    },
    true,
  );
})();
