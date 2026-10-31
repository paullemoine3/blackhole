// Injected at document start into every extension page (background, popup).
// WebKit has no `privacy` permission nor `privacy.services`, which password managers use to
// switch off the browser's own autofill; the "make Bitwarden the default" setting then never
// sticks. Emulate them: the value is remembered in the extension's own storage.
(() => {
  const KEY = '__bhPrivacy';
  const NAMES = ['passwordSavingEnabled', 'autofillAddressEnabled', 'autofillCreditCardEnabled'];
  const isPrivacy = (d) => d && Array.isArray(d.permissions) && d.permissions.includes('privacy');
  const cleaned = (d) => {
    const rest = d.permissions.filter((p) => p !== 'privacy');
    return { ...d, permissions: rest };
  };

  const install = (api) => {
    if (!api || !api.permissions || api.permissions.__bhShim) return false;
    const area = api.storage && api.storage.local;
    const read = () => new Promise((ok) => area.get(KEY, (r) => ok((r && r[KEY]) || {})));
    const write = (v) => new Promise((ok) => area.set({ [KEY]: v }, ok));
    const native = api.permissions;
    try { Object.defineProperty(native, '__bhShim', { value: true }); } catch (_) {}
    const orig = { contains: native.contains.bind(native), request: native.request.bind(native) };
    const patched = {};
    for (const name of ['contains', 'request']) {
      patched[name] = (details, cb) => {
        if (!isPrivacy(details)) return orig[name](details, cb);
        const rest = cleaned(details);
        const done = (v) => (cb ? cb(v) : Promise.resolve(v));
        // Origins and other permissions are still delegated to the engine.
        if (!rest.permissions.length && !(rest.origins && rest.origins.length)) return done(true);
        return orig[name](rest, cb);
      };
    }
    const services = {};
    for (const name of NAMES) {
      services[name] = {
        get: (_details, cb) => {
          const p = read().then((s) => ({
            value: name in s ? s[name] : true,
            levelOfControl: name in s ? 'controlled_by_this_extension' : 'controllable_by_this_extension',
          }));
          return cb ? void p.then(cb) : p;
        },
        set: (details, cb) => {
          const p = read()
            .then((s) => write({ ...s, [name]: !!(details && details.value) }))
            .then(() => undefined);
          return cb ? void p.then(() => cb()) : p;
        },
        clear: (_d, cb) => {
          const p = read()
            .then((s) => {
              delete s[name];
              return write(s);
            })
            .then(() => undefined);
          return cb ? void p.then(() => cb()) : p;
        },
      };
    }
    const privacy = { services };
    for (const name of ['contains', 'request']) {
      Reflect.defineProperty(native, name, { value: patched[name], configurable: true, writable: true });
    }
    Reflect.defineProperty(api, 'privacy', { value: privacy, configurable: true });
    return true;
  };

  const run = () => {
    for (const name of ['chrome', 'browser']) {
      try {
        const api = window[name];
        if (!api || !api.permissions || api.__bhShim) continue;
        install(api);
      } catch (_) {}
    }
  };
  run();
  document.addEventListener('DOMContentLoaded', run);
})();
