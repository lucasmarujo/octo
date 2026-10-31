const CACHE = 'orkai-pwa-v3';
const SHELL = ['/', '/app.js', '/wire.js', '/style.css', '/manifest.webmanifest', '/octo.png', '/kai/idle.svg', '/kai/working.svg', '/kai/needs.svg', '/kai/done.svg', '/kai/lost.svg', '/fonts/mono-400.woff2', '/fonts/mono-700.woff2'];
const LIVE = ['/ws', '/health'];
const UNREACHABLE = 500;

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(SHELL)).then(() => self.skipWaiting()));
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener('fetch', (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (request.method !== 'GET' || url.origin !== self.location.origin || LIVE.includes(url.pathname)) return;
  event.respondWith(
    fetch(request)
      .then((response) => {
        if (response.ok) {
          const copy = response.clone();
          caches.open(CACHE).then((cache) => cache.put(request, copy));
        }
        if (response.status < UNREACHABLE) return response;
        return caches.match(request, { ignoreSearch: true }).then((cached) => cached || response);
      })
      .catch(() => caches.match(request, { ignoreSearch: true }).then((cached) => cached || caches.match('/'))),
  );
});
