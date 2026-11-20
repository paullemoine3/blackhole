// Fingerprinting protection (Shields › Fingerprinting, `domain::fingerprint`). What a page reads
// back to tell this device from others varies by site and by session, while staying the same
// for the whole visit so pages keep working: a few pixels of canvas and WebGL readbacks, audio
// samples off by an inaudible amount, a generic graphics card, a rounded core count. Drawing,
// playing and the pages' own rendering are untouched. Runs in every frame, before the page.
(() => {
  const config = __FINGERPRINT__;
  if (!config || !/^https?:$/.test(location.protocol)) return;
  // The site of the tab, frames included: a tracker's frame embedded on two sites must not see
  // the same values there.
  let top = location.hostname;
  try {
    const anc = location.ancestorOrigins;
    if (anc && anc.length) top = new URL(anc[anc.length - 1]).hostname;
  } catch (_) {}
  top = top.toLowerCase();
  if (config.exempt.some((site) => top === site || top.endsWith('.' + site))) return;
  const site = top.split('.').slice(-2).join('.');

  // A seed of this site for this session (FNV-1a), and a stable pseudo-random value per index.
  let seed = 0x811c9dc5;
  for (const ch of config.secret + '|' + site) {
    seed ^= ch.charCodeAt(0);
    seed = Math.imul(seed, 0x01000193) >>> 0;
  }
  const mix = (n) => {
    let x = (seed ^ Math.imul(n | 0, 0x9e3779b1)) >>> 0;
    x ^= x << 13;
    x ^= x >>> 17;
    x ^= x << 5;
    return x >>> 0;
  };

  // A replaced function still reads as native code (`toString` of a Proxy) and keeps its name.
  const wrap = (proto, name, apply) => {
    if (!proto) return;
    const desc = Object.getOwnPropertyDescriptor(proto, name);
    if (!desc || typeof desc.value !== 'function') return;
    const original = desc.value;
    try {
      Object.defineProperty(proto, name, { ...desc, value: new Proxy(original, { apply: (target, self, args) => apply(target, self, args) }) });
    } catch (_) {}
  };
  const getter = (proto, name, value) => {
    if (!proto) return;
    const desc = Object.getOwnPropertyDescriptor(proto, name);
    if (!desc || !desc.get) return;
    const original = desc.get;
    try {
      Object.defineProperty(proto, name, { ...desc, get: new Proxy(original, { apply: (target, self, args) => value(Reflect.apply(target, self, args)) }) });
    } catch (_) {}
  };

  // Flips the lowest bit of one colour value every 199 pixels, from a place of the site's own:
  // a few thousand values on a full screen, nothing a person sees, and cheap for a page that
  // reads its canvas every frame. Byte pixels only (a float readback is left as it is).
  const perturb = (data) => {
    if (!(data instanceof Uint8ClampedArray || data instanceof Uint8Array) || !data.length) return data;
    for (let i = (seed % 199) * 4; i < data.length; i += 199 * 4) {
      data[i + (mix(i) % 3)] ^= 1;
    }
    return data;
  };

  // Canvas: readbacks come from a varied copy; the canvas itself is never changed.
  const getImageData = CanvasRenderingContext2D.prototype.getImageData;
  const varied = (canvas) => {
    const w = canvas.width;
    const h = canvas.height;
    if (!w || !h || w * h > 4000000) return null;
    try {
      const copy = document.createElement('canvas');
      copy.width = w;
      copy.height = h;
      const ctx = copy.getContext('2d');
      ctx.drawImage(canvas, 0, 0);
      const image = getImageData.call(ctx, 0, 0, w, h);
      perturb(image.data);
      ctx.putImageData(image, 0, 0);
      return copy;
    } catch (_) {
      return null;
    }
  };
  for (const name of ['toDataURL', 'toBlob']) {
    wrap(HTMLCanvasElement.prototype, name, (target, self, args) => Reflect.apply(target, varied(self) || self, args));
  }
  wrap(CanvasRenderingContext2D.prototype, 'getImageData', (target, self, args) => {
    const image = Reflect.apply(target, self, args);
    perturb(image.data);
    return image;
  });

  // WebGL: a generic graphics card, and varied pixel readbacks.
  for (const gl of [window.WebGLRenderingContext, window.WebGL2RenderingContext]) {
    if (!gl) continue;
    wrap(gl.prototype, 'getParameter', (target, self, args) => {
      if (args[0] === 0x9245) return 'WebKit'; // UNMASKED_VENDOR_WEBGL
      if (args[0] === 0x9246) return 'WebKit WebGL'; // UNMASKED_RENDERER_WEBGL
      return Reflect.apply(target, self, args);
    });
    wrap(gl.prototype, 'readPixels', (target, self, args) => {
      const out = Reflect.apply(target, self, args);
      const pixels = args[6];
      if (pixels && ArrayBuffer.isView(pixels)) perturb(pixels);
      return out;
    });
  }

  // Audio: rendered samples read back are off by far less than anyone can hear (-140 dB).
  if (window.AudioBuffer) {
    const done = new WeakSet();
    wrap(AudioBuffer.prototype, 'getChannelData', (target, self, args) => {
      const data = Reflect.apply(target, self, args);
      if (!done.has(data)) {
        done.add(data);
        for (let i = 0; i < data.length; i += 97) data[i] += ((mix(i) % 2001) - 1000) * 1e-10;
      }
      return data;
    });
  }
  if (window.AnalyserNode) {
    wrap(AnalyserNode.prototype, 'getFloatFrequencyData', (target, self, args) => {
      const out = Reflect.apply(target, self, args);
      const data = args[0];
      if (data) for (let i = 0; i < data.length; i += 7) data[i] += ((mix(i) % 201) - 100) * 1e-6;
      return out;
    });
  }

  // The device: a rounded core count, no battery level.
  const nav = window.Navigator && Navigator.prototype;
  getter(nav, 'hardwareConcurrency', (n) => (n >= 8 ? 8 : n >= 4 ? 4 : 2));
  if (nav && 'getBattery' in nav) {
    try {
      delete Navigator.prototype.getBattery;
    } catch (_) {}
  }
})();
