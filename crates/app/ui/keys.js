// Shortcut notation for BlackHole's own pages, injected in each of them: one spelling everywhere,
// the one of the system. `bhKeys('mod+shift+e')` is "⌘⇧E" on macOS and "Ctrl+Shift+E" elsewhere;
// `bhKeyParts` gives the same keys one by one (for <kbd> chips).
(() => {
  if (window.bhKeys) return;
  const mac = /mac/i.test(navigator.platform);
  const NAMES = mac
    ? { mod: '⌘', shift: '⇧', alt: '⌥', ctrl: '⌃', enter: '↵', esc: 'Esc', tab: 'Tab', plus: '+', minus: '−', left: '←', right: '→', up: '↑', down: '↓' }
    : { mod: 'Ctrl', shift: 'Shift', alt: 'Alt', ctrl: 'Ctrl', enter: 'Enter', esc: 'Esc', tab: 'Tab', plus: '+', minus: '−', left: '←', right: '→', up: '↑', down: '↓' };
  const name = (k) => NAMES[k.toLowerCase()] || (k.length === 1 ? k.toUpperCase() : k);
  const parts = (spec) => spec.split('+').map(name);
  Object.defineProperty(window, 'bhMac', { value: mac });
  Object.defineProperty(window, 'bhKeyParts', { value: parts });
  Object.defineProperty(window, 'bhKeys', { value: (spec) => parts(spec).join(mac ? '' : '+') });
})();
