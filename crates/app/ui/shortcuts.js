// Injected in every BlackHole view: native web views swallow key events, so app shortcuts
// are forwarded to the shell from inside the page.
(() => {
  if (!window.ipc || window.top !== window) return;
  const mac = navigator.platform.toLowerCase().includes('mac');
  const KEYS = ['l', 't', 'w', 'r', 'b', '[', ']', ',', '\\', 'enter', 'n'];
  addEventListener(
    'keydown',
    (e) => {
      // Escape is only forwarded (never swallowed): it closes the palette or a peek.
      if (e.key === 'Escape') return window.ipc.postMessage(JSON.stringify({ cmd: 'shortcut', key: 'escape' }));
      if ((mac ? !e.metaKey : !e.ctrlKey) || e.altKey) return;
      let key = e.key.toLowerCase();
      if (!KEYS.includes(key)) return;
      // Shift only matters for new windows: Mod+Shift+N opens a private one.
      if (e.shiftKey) {
        if (key !== 'n') return;
        key = 'shift+n';
      } else if (key === 'n' && window.keepCtrlN) return;
      e.preventDefault();
      e.stopPropagation();
      window.ipc.postMessage(JSON.stringify({ cmd: 'shortcut', key }));
    },
    true,
  );
})();
