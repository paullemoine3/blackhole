// BlackHole: the keys `performKeyEquivalent:` gives the page before the menu (class/wry_web_view.rs).
// When the page left the key alone, WebKit's own undo, redo or selection runs, in the frame that
// has the focus. Checked once every listener has run, so a page may stop the event's propagation.
(() => {
  addEventListener(
    'keydown',
    (e) => {
      if (!e.isTrusted || !e.metaKey || e.ctrlKey || e.altKey) return;
      const key = e.key.toLowerCase();
      const command = key === 'z' ? (e.shiftKey ? 'redo' : 'undo') : key === 'a' && !e.shiftKey ? 'selectAll' : '';
      if (command) setTimeout(() => e.defaultPrevented || document.execCommand(command), 0);
    },
    true,
  );
})();
