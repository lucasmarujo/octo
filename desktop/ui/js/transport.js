const core = () => window.__TAURI__.core;

export async function connect(onMessage) {
  const channel = new (core().Channel)();
  channel.onmessage = onMessage;
  await core().invoke('attach', { channel });
}

export function send(message) {
  return core()
    .invoke('send', { message })
    .catch((err) => {
      window.dispatchEvent(new CustomEvent('orkai:error', { detail: String(err) }));
    });
}

export function currentWindow() {
  return window.__TAURI__.window.getCurrentWindow();
}
