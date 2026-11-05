// Native login autofill (main frame only). The page never sees a password until the user
// clicks a suggestion: it only receives the usernames, drawn in a closed shadow root.
(() => {
  if (!window.__bhipc || window.top !== window || window.__bhAutofill) return;
  const post = (m) => {
    try {
      window.__bhipc.postMessage(JSON.stringify(m));
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
  root.innerHTML = `<style>
    .m{position:fixed;min-width:220px;max-width:340px;background:#1c1b26;color:#eee;border:1px solid #3a3950;
       border-radius:10px;box-shadow:0 8px 28px #0008;font:13px -apple-system,system-ui,sans-serif;padding:4px;display:none}
    .h{padding:4px 8px;font-size:11px;color:#9b99b5}
    .i{padding:7px 8px;border-radius:6px;cursor:pointer;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
    .i:hover{background:#34325a}
  </style><div class="m"></div>`;
  const menu = root.querySelector('.m');
  let target = null;
  let names = [];

  const place = () => {
    if (!target || menu.style.display === 'none') return;
    const r = target.getBoundingClientRect();
    menu.style.left = `${Math.max(4, Math.min(r.left, innerWidth - 240))}px`;
    menu.style.top = `${r.bottom + 4}px`;
  };
  const hide = () => {
    menu.style.display = 'none';
  };
  const show = (list) => {
    names = list;
    if (!target || !list.length) return hide();
    menu.replaceChildren();
    const h = document.createElement('div');
    h.className = 'h';
    h.textContent = 'BlackHole · identifiants enregistrés';
    menu.append(h);
    list.forEach((name, index) => {
      const row = document.createElement('div');
      row.className = 'i';
      row.textContent = name || '(sans identifiant)';
      // Keep focus in the page field while clicking.
      row.addEventListener('mousedown', (e) => e.preventDefault());
      row.addEventListener('click', (e) => {
        if (!e.isTrusted) return;
        post({ cmd: 'autofill_fill', index });
        hide();
      });
      menu.append(row);
    });
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

  document.addEventListener(
    'focusin',
    (e) => {
      if (!isLoginField(e.target)) return hide();
      target = e.target;
      post({ cmd: 'autofill_query' });
    },
    true,
  );
  document.addEventListener('focusout', () => setTimeout(hide, 150), true);
  document.addEventListener('keydown', (e) => e.key === 'Escape' && hide(), true);
  addEventListener('scroll', place, true);
  addEventListener('resize', place);

  Object.defineProperty(window, '__bhAutofill', {
    value: Object.freeze({ show, fill }),
    enumerable: false,
  });
})();
