const core = () => window.__TAURI__.core;

export function channel(onMessage) {
  const created = new (core().Channel)();
  created.onmessage = onMessage;
  return created;
}

export async function connect(onMessage) {
  await core().invoke('attach', { channel: channel(onMessage) });
}

export function send(message) {
  return core()
    .invoke('send', { message })
    .catch((err) => {
      window.dispatchEvent(new CustomEvent('orkai:error', { detail: String(err) }));
    });
}

export function invoke(command, args) {
  return core().invoke(command, args);
}

export function currentWindow() {
  return window.__TAURI__.window.getCurrentWindow();
}
