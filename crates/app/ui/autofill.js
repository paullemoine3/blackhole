// Native login autofill (main frame only). The page never sees a password until the user
// clicks a suggestion: it only receives the usernames, drawn in a closed shadow root. The shell
// re-reads the page's origin itself and only serves https (or localhost) pages. After a login
// form is submitted, its credentials (already the page's) are handed to the shell, which offers
// to save them in the sidebar.
(() => {
  // Texts in the interface language, set when the page's view is created (views::page_script).
  const TEXT = __BH_TEXT__;
  const T = (key) => TEXT[key] || key;
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

  // An email field an alias suits: sign-ups, newsletters. Not a sign-in, where the address is
  // already known (`current-password` in the form, or a `username` field of its own).
  const isAliasField = (el) => {
    if (!(el instanceof HTMLInputElement) || el.readOnly || el.disabled || !/^(text|email|)$/.test(el.type)) return false;
    const auto = el.autocomplete || '';
    const hint = /e-?mail|courriel/i;
    if (el.type !== 'email' && !/\bemail\b/.test(auto) && !hint.test(`${el.name} ${el.id} ${el.placeholder}`)) return false;
    const scope = scopeOf(el);
    const passwords = scope.querySelectorAll ? [...scope.querySelectorAll('input[type=password]')].filter(visible) : [];
    if (passwords.some((i) => /current-password/.test(i.autocomplete || ''))) return false;
    const fresh = passwords.length > 1 || passwords.some((i) => /new-password/.test(i.autocomplete || ''));
    return fresh || !/\busername\b/.test(auto);
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
    .i:hover,.i.on{background:#34325a}
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
  menu.setAttribute('role', 'listbox');
  root.appendChild(menu);
  let target = null;
  // The field an alias was asked for, and the token of its menu: focus may move meanwhile.
  let aliasFor = null;
  let written = null;
  // The entries on screen, with what each one sends; `active` is the one the arrow keys reached
  // (-1: none, so Enter still belongs to the page's form).
  let rows = [];
  let active = -1;
  let shown = false;

  const place = () => {
    if (!target || menu.style.display === 'none') return;
    const r = target.getBoundingClientRect();
    menu.style.left = `${Math.max(4, Math.min(r.left, innerWidth - 240))}px`;
    menu.style.top = `${r.bottom + 4}px`;
  };
  const hide = () => {
    menu.style.display = 'none';
    shown = false;
    active = -1;
  };
  const mark = (i) => {
    active = i;
    rows.forEach((r, k) => {
      r.row.classList.toggle('on', k === i);
      r.row.setAttribute('aria-selected', String(k === i));
    });
  };
  const item = (label, message) => {
    const row = document.createElement('div');
    row.className = 'i';
    row.setAttribute('role', 'option');
    row.setAttribute('aria-selected', 'false');
    row.textContent = label;
    rows.push({ row, message });
    // Keep focus in the page field while clicking.
    apply(listen, row, ['mousedown', (e) => e.preventDefault()]);
    apply(listen, row, [
      'click',
      (e) => {
        if (!e.isTrusted) return;
        pick(message);
      },
    ]);
    menu.append(row);
  };
  const head = (text) => {
    const h = document.createElement('div');
    h.className = 'h';
    h.textContent = text;
    h.setAttribute('role', 'presentation');
    menu.append(h);
  };
  const pick = (message) => {
    post(message);
    if (message.cmd !== 'autofill_alias') return hide();
    aliasFor = { field: target, token: message.token };
    if ('index' in message) return hide();
    // The service takes a moment: the menu says so until the alias is written.
    rows = [];
    active = -1;
    menu.replaceChildren();
    head(T('page.alias_making'));
  };
  // `m`: { names: usernames saved for this origin, locked, generate, token }.
  const show = (m) => {
    const list = (m && m.names) || [];
    const token = (m && m.token) || '';
    if (!target || (!list.length && !m.locked && !m.generate && !m.alias && !(m.aliases || []).length)) return hide();
    menu.replaceChildren();
    rows = [];
    active = -1;
    const saved = list.length || m.locked || m.generate;
    menu.setAttribute('aria-label', T(saved ? 'page.saved_passwords' : 'page.alias_head'));
    if (saved) head(T('page.saved_passwords'));
    for (let index = 0; index < list.length; index++) {
      item(list[index] || T('settings.no_username'), { cmd: 'autofill_fill', index, token });
    }
    if (m.locked) item(T('page.unlock_passwords'), { cmd: 'autofill_unlock', token });
    if (m.generate) item(T('page.strong_password'), { cmd: 'autofill_generate', token });
    const aliases = m.aliases || [];
    if (aliases.length || m.alias) head(m.alias ? `${T('page.alias_head')} · ${m.alias}` : T('page.alias_head'));
    for (let index = 0; index < aliases.length; index++) item(aliases[index], { cmd: 'autofill_alias', index, token });
    if (m.alias) item(T('page.alias_create'), { cmd: 'autofill_alias', token });
    menu.style.display = 'block';
    shown = true;
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
  // The alias asked for (empty: it could not be made).
  const alias = (token, email) => {
    if (!aliasFor || aliasFor.token !== token) return;
    const field = aliasFor.field;
    aliasFor = null;
    hide();
    if (!field || !field.isConnected || !email) return;
    setValue(field, email);
    written = field;
  };
  const aliasGone = (email) => {
    if (written && written.isConnected && written.value === email) setValue(written, '');
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
      const login = isLoginField(e.target);
      const email = isAliasField(e.target);
      if (!login && !email) return hide();
      target = e.target;
      post({ cmd: 'autofill_query', fresh: isFresh(target), login, email });
    },
    true,
  );
  document.addEventListener('focusout', () => setTimeout(hide, 150), true);
  // The keyboard, as in every browser: ↓ / ↑ walk the entries from the field, Enter picks the
  // one reached (a real key press, `isTrusted`, like the click), Escape closes the menu.
  document.addEventListener(
    'keydown',
    (e) => {
      if (e.key === 'Escape') return hide();
      if (!shown || !e.isTrusted || e.target !== target || !rows.length) return;
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        const n = rows.length;
        mark(e.key === 'ArrowDown' ? (active + 1) % n : (active <= 0 ? n : active) - 1);
      } else if (e.key === 'Enter' && active >= 0) {
        pick(rows[active].message);
      } else return;
      e.preventDefault();
      e.stopImmediatePropagation();
    },
    true,
  );
  addEventListener('scroll', place, true);
  addEventListener('resize', place);

  Object.defineProperty(window, '__bhAutofill', {
    value: Object.freeze({ show, fill, fillNew, alias, aliasGone }),
    enumerable: false,
  });
})();
