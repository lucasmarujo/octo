export const $ = (id) => document.getElementById(id);

export function h(tag, props, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props || {})) {
    if (value === undefined || value === null || value === false) continue;
    if (key === 'class') node.className = value;
    else if (key === 'text') node.textContent = value;
    else if (key === 'html') node.innerHTML = value;
    else if (key === 'style') node.style.cssText = value;
    else if (key === 'dataset') Object.assign(node.dataset, value);
    else if (key.startsWith('on')) node.addEventListener(key.slice(2), value);
    else node.setAttribute(key, value === true ? '' : value);
  }
  for (const child of children.flat()) {
    if (child === null || child === undefined || child === false) continue;
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
}

export function keep(cache, key, signature, build) {
  const hit = cache.get(key);
  if (hit && hit.signature === signature) return hit.node;
  const node = build();
  cache.set(key, { signature, node });
  return node;
}

export function place(parent, nodes) {
  const same = parent.children.length === nodes.length && nodes.every((node, index) => parent.children[index] === node);
  if (!same) parent.replaceChildren(...nodes);
}

export function svg(markup) {
  const holder = document.createElement('template');
  holder.innerHTML = markup.trim();
  return holder.content.firstElementChild;
}

export const storage = {
  get(key) {
    try {
      return window.localStorage.getItem(key);
    } catch (err) {
      return null;
    }
  },
  set(key, value) {
    try {
      if (value === null) window.localStorage.removeItem(key);
      else window.localStorage.setItem(key, value);
    } catch (err) {
      return;
    }
  },
  json(key, fallback) {
    const raw = storage.get(key);
    if (raw === null) return fallback;
    try {
      return JSON.parse(raw);
    } catch (err) {
      return fallback;
    }
  },
};
