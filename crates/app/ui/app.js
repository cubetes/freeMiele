// Draws the cards the Rust side sends. No logic about Miele here.
const tauri = window.__TAURI__;
const list = document.getElementById('cards');

function el(tag, className, text) {
  const e = document.createElement(tag);
  if (className) e.className = className;
  if (text) e.textContent = text;
  return e;
}

function render(cards) {
  list.replaceChildren(...cards.map((c) => {
    const card = el('article', 'card');
    const meta = el('div', 'meta');
    meta.append(el('span', 'label', c.label), el('span', `chip ${c.tone}`, c.chip));
    const text = el('div');
    text.append(el('div', 'title', c.title));
    if (c.sub) text.append(el('div', 'sub', c.sub));
    card.append(meta, text);
    if (c.progress != null) {
      const bar = el('div', 'track');
      bar.setAttribute('role', 'progressbar');
      bar.setAttribute('aria-valuenow', c.progress);
      bar.setAttribute('aria-valuemin', 0);
      bar.setAttribute('aria-valuemax', 100);
      const fill = el('div');
      fill.style.width = `${c.progress}%`;
      bar.append(fill);
      card.append(bar);
    }
    return card;
  }));
}

if (tauri) {
  document.getElementById('more').addEventListener('click', () => tauri.core.invoke('more'));
  // The window is as tall as the panel.
  new ResizeObserver(() => tauri.core.invoke('fit', { height: document.querySelector('.panel').offsetHeight }))
    .observe(document.querySelector('.panel'));
  tauri.event.listen('cards', (e) => render(e.payload));
  tauri.core.invoke('cards').then(render);
}
