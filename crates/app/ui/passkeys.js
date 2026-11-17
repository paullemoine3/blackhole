// Passkeys in web pages (macOS only). WebKit refuses WebAuthn to a browser without Apple's
// browser entitlement (`domain::passkeys`): a request is rejected at once with nothing on screen,
// while `isUserVerifyingPlatformAuthenticatorAvailable` takes seconds to say no and
// `getClientCapabilities` claims a platform authenticator. Sites then offer a passkey that cannot
// work, and the extensions that serve passkeys (Bitwarden) wait for that slow "no" before they
// step in.
// So, before any page or extension script: the page is told at once that there is no platform
// authenticator, and a passkey request the system refused is reported to the shell (it explains
// it in a notice). Extensions that replace `navigator.credentials` keep doing so and fall back on
// these. Nothing else of the page is touched.
(() => {
  const PKC = window.PublicKeyCredential;
  // On the prototype: WebKit gives the page a new `navigator.credentials` after document start,
  // so what is set on the first one is lost.
  const proto = window.CredentialsContainer && window.CredentialsContainer.prototype;
  if (!PKC || !proto || !/^https?:$/.test(location.protocol)) return;
  const no = () => Promise.resolve(false);
  const set = (obj, name, fn) => { try { Object.defineProperty(obj, name, { value: fn, writable: true, configurable: true }); } catch (_) {} };
  set(PKC, 'isUserVerifyingPlatformAuthenticatorAvailable', no);
  if (typeof PKC.isConditionalMediationAvailable === 'function') set(PKC, 'isConditionalMediationAvailable', no);
  if (typeof PKC.getClientCapabilities === 'function') {
    const caps = PKC.getClientCapabilities.bind(PKC);
    const none = { passkeyPlatformAuthenticator: false, userVerifyingPlatformAuthenticator: false,
      conditionalGet: false, conditionalCreate: false, conditionalMediation: false, hybridTransport: false };
    set(PKC, 'getClientCapabilities', () => caps().then((c) => Object.assign({}, c, none), () => Object.assign({}, none)));
  }
  // The main frame alone tells the shell (one notice per site, decided there).
  const tell = () => {
    if (window !== window.top) return;
    try { window.__bhipc && window.__bhipc.postMessage(JSON.stringify({ cmd: 'passkey_refused' })); } catch (_) {}
  };
  const watch = (call) => function (options) {
    const started = Date.now();
    const p = call.call(this, options);
    if (!options || !options.publicKey || options.mediation === 'conditional') return p;
    // Refused by the system: at once. A person who closes a prompt takes longer than that.
    return p.catch((e) => {
      if (e && e.name === 'NotAllowedError' && Date.now() - started < 1500) tell();
      throw e;
    });
  };
  if (typeof proto.get === 'function') set(proto, 'get', watch(proto.get));
  if (typeof proto.create === 'function') set(proto, 'create', watch(proto.create));
})();
