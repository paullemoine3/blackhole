// Lets the shell see and drive the camera and microphone a page opened (video calls), and tells
// it when a video leaves Picture in Picture (the "back to tab" button of the floating window).
(() => {
  if (!/^https?:$/.test(location.protocol)) return;
  const post = (message) => {
    try {
      window.ipc && window.ipc.postMessage(JSON.stringify(message));
    } catch (_) {}
  };

  const tracks = new Set();
  const md = navigator.mediaDevices;
  if (md && md.getUserMedia) {
    const original = md.getUserMedia.bind(md);
    md.getUserMedia = async (constraints) => {
      const stream = await original(constraints);
      stream.getTracks().forEach((t) => tracks.add(t));
      return stream;
    };
  }
  const live = (kind) => {
    for (const t of tracks) if (t.readyState === 'ended') tracks.delete(t);
    return [...tracks].filter((t) => t.kind === kind);
  };
  window.__bhRtc = {
    state() {
      const cam = live('video');
      const mic = live('audio');
      return {
        cam: cam.length > 0,
        mic: mic.length > 0,
        camOn: cam.some((t) => t.enabled),
        micOn: mic.some((t) => t.enabled),
      };
    },
    toggle(kind) {
      const list = live(kind);
      const enable = !list.some((t) => t.enabled);
      list.forEach((t) => (t.enabled = enable));
    },
  };

  const left = (video) => post({ cmd: 'pip_left', paused: !!video.paused });
  document.addEventListener(
    'leavepictureinpicture',
    (e) => e.target instanceof HTMLVideoElement && left(e.target),
    true
  );
  document.addEventListener(
    'webkitpresentationmodechanged',
    (e) => {
      const v = e.target;
      if (!(v instanceof HTMLVideoElement)) return;
      if (v.webkitPresentationMode === 'picture-in-picture') v.__bhPip = true;
      else if (v.__bhPip) {
        v.__bhPip = false;
        left(v);
      }
    },
    true
  );
})();
