// Native login autofill (main frame only). The page never sees a password until the user
// clicks a suggestion: it only receives the usernames, drawn in a closed shadow root. The shell
// re-reads the page's origin itself and only serves https (or localhost) pages. After a login
// form is submitted, its credentials (already the page's) are handed to the shell, which offers
// to save them in the sidebar.
(() => {
  if (!window.__bhipc || window.top !== window || window.__bhAutofill) return;
  // Page scripts run after this one and may patch anything global: what carries the menu's
  // one-time token (the proof that the user picked an entry) is captured now. The shell only
  // fills a password for a message bearing the token it put in the last menu.
  const apply = Reflect.apply;
  const stringify = JSON.stringify;
  const listen = EventTarget.prototype.addEventListener;
  const bridge =
    (window.chrome && window.chrome.webview) ||
    (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.ipc) ||
    window.__bhipc;
  const bridgePost = bridge.postMessage;
  const post = (m) => {
    try {
      apply(bridgePost, bridge, [stringify(m)]);
    } catch (_) {}
  };
  const setValue = (el, value) => {
    const proto = Object.getPrototypeOf(el);
    const setter = Object.getOwnPropertyDescriptor(proto, 'value');
    if (setter && setter.set) setter.set.call(el, value);
    else el.value = value;
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  };
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== 'hidden';
  };
  const scopeOf = (el) => el.form || el.getRootNode();
  const passwordIn = (scope) =>
    [...scope.querySelectorAll('input[type=password]')].find(visible) || null;
  const usernameFor = (pw, scope) => {
    const all = [...scope.querySelectorAll('input')].filter(visible);
    const before = all.slice(0, all.indexOf(pw)).reverse();
    return (
      before.find((i) => /^(text|email|tel|)$/.test(i.type) && !i.readOnly && !i.disabled) || null
    );
  };
  // A field qualifies when it is a password input or the username input of a login form.
  const isLoginField = (el) => {
    if (!(el instanceof HTMLInputElement) || el.readOnly || el.disabled) return false;
    const scope = scopeOf(el);
    if (el.type === 'password') return !!scope.querySelector;
    if (!/^(text|email|tel|)$/.test(el.type)) return false;
    const pw = scope.querySelector && passwordIn(scope);
    return !!pw && usernameFor(pw, scope) === el;
  };

  const host = document.createElement('div');
  host.style.cssText = 'all:initial;position:fixed;z-index:2147483647;top:0;left:0;';
  const root = host.attachShadow({ mode: 'closed' });
  // Built node by node: pages under Trusted Types (`require-trusted-types-for 'script'`, e.g.
  // accounts.google.com) reject any string assigned to `innerHTML`, even our own.
  const CSS_TEXT = `    .m{position:fixed;min-width:220px;max-width:340px;background:#1c1b26;color:#eee;border:1px solid #3a3950;
       border-radius:10px;box-shadow:0 8px 28px #0008;font:13px -apple-system,system-ui,sans-serif;padding:4px;display:none}
    .h{padding:4px 8px;font-size:11px;color:#9b99b5}
    .i{padding:7px 8px;border-radius:6px;cursor:pointer;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
    .i:hover{background:#34325a}
  `;
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(CSS_TEXT);
    root.adoptedStyleSheets = [sheet];
  } catch (_) {
    const style = document.createElement('style');
    style.textContent = CSS_TEXT;
    root.appendChild(style);
  }
  const menu = document.createElement('div');
  menu.className = 'm';
  root.appendChild(menu);
  let target = null;

  const place = () => {
    if (!target || menu.style.display === 'none') return;
    const r = target.getBoundingClientRect();
    menu.style.left = `${Math.max(4, Math.min(r.left, innerWidth - 240))}px`;
    menu.style.top = `${r.bottom + 4}px`;
  };
  const hide = () => {
    menu.style.display = 'none';
  };
  const item = (label, message) => {
    const row = document.createElement('div');
    row.className = 'i';
    row.textContent = label;
    // Keep focus in the page field while clicking.
    apply(listen, row, ['mousedown', (e) => e.preventDefault()]);
    apply(listen, row, [
      'click',
      (e) => {
        if (!e.isTrusted) return;
        post(message);
        hide();
      },
    ]);
    menu.append(row);
  };
  // `m`: { names: usernames saved for this origin, locked, generate, token }.
  const show = (m) => {
    const list = (m && m.names) || [];
    const token = (m && m.token) || '';
    if (!target || (!list.length && !m.locked && !m.generate)) return hide();
    menu.replaceChildren();
    const h = document.createElement('div');
    h.className = 'h';
    h.textContent = 'BlackHole · saved passwords';
    menu.append(h);
    for (let index = 0; index < list.length; index++) {
      item(list[index] || '(no username)', { cmd: 'autofill_fill', index, token });
    }
    if (m.locked) item('Unlock saved passwords…', { cmd: 'autofill_unlock', token });
    if (m.generate) item('Use a strong password', { cmd: 'autofill_generate', token });
    menu.style.display = 'block';
    if (!host.isConnected) document.documentElement.append(host);
    place();
  };

  const fill = (user, pass) => {
    if (!target) return;
    const scope = scopeOf(target);
    const pw = target.type === 'password' ? target : passwordIn(scope);
    const un = target.type === 'password' ? usernameFor(pw, scope) : target;
    if (un && user) setValue(un, user);
    if (pw) setValue(pw, pass);
  };
  // A generated password goes into every new-password field of the form (password + confirm).
  const fillNew = (pass) => {
    if (!target) return;
    const scope = scopeOf(target);
    const fresh = [...scope.querySelectorAll('input[type=password]')].filter(
      (i) => visible(i) && /new-password/.test(i.autocomplete || ''),
    );
    (fresh.length ? fresh : [target]).forEach((i) => setValue(i, pass));
  };
  const isFresh = (el) => el.type === 'password' && /new-password/.test(el.autocomplete || '');

  const SIGN_IN = /\b(log ?in|sign ?in|connexion|se connecter|continue|continuer|next|suivant|submit|valider)\b/i;
  // Submission: a form submit, Enter in a field, or a click on a button next to a password.
  let lastSent = '';
  const submitted = (scope) => {
    if (!scope || !scope.querySelectorAll) return;
    const pw = [...scope.querySelectorAll('input[type=password]')].find((i) => visible(i) && i.value);
    if (!pw) return;
    const un = usernameFor(pw, scope);
    const username = (un && un.value) || '';
    const key = username + '\u0000' + pw.value;
    if (key === lastSent) return;
    lastSent = key;
    setTimeout(() => { if (lastSent === key) lastSent = ''; }, 3000);
    post({ cmd: 'login_submitted', username, password: pw.value });
  };
  document.addEventListener('submit', (e) => e.isTrusted && submitted(e.target), true);
  document.addEventListener(
    'keydown',
    (e) => {
      if (e.isTrusted && e.key === 'Enter' && e.target instanceof HTMLInputElement) submitted(scopeOf(e.target));
    },
    true,
  );
  document.addEventListener(
    'click',
    (e) => {
      if (!e.isTrusted) return;
      const b = e.target instanceof Element && e.target.closest('button, input[type=submit], [role=button]');
      if (!b) return;
      // In a form only its submit buttons count (not "show password"); elsewhere (script-driven
      // sign-in pages) a button that reads like one.
      const form = b.form || b.closest('form');
      if (form ? b.type === 'submit' : SIGN_IN.test(b.textContent || b.value || '')) submitted(form || b.getRootNode());
    },
    true,
  );

  document.addEventListener(
    'focusin',
    (e) => {
      if (!isLoginField(e.target)) return hide();
      target = e.target;
      post({ cmd: 'autofill_query', fresh: isFresh(target) });
    },
    true,
  );
  document.addEventListener('focusout', () => setTimeout(hide, 150), true);
  document.addEventListener('keydown', (e) => e.key === 'Escape' && hide(), true);
  addEventListener('scroll', place, true);
  addEventListener('resize', place);

  Object.defineProperty(window, '__bhAutofill', {
    value: Object.freeze({ show, fill, fillNew }),
    enumerable: false,
  });
})();
