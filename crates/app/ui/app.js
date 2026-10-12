// Draws the cards the Rust side sends. No logic about Miele here.
const tauri = window.__TAURI__;
const list = document.getElementById('cards');
// Coffee controls, kept by device id so a live update (which redraws everything) loses nothing.
const drinks = new Map(); // id -> { favourites, all }; null while loading
const picked = new Map(); // id -> { id, name }
const busy = new Set(); // ids with a request in flight
const notes = new Map(); // id -> result text, shown for a few seconds
const timers = new Map();
let last = [];
let menuFor = null;
const TICK = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" style="color:var(--brand)"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>';
const POWER = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M12 3v8"/><path d="M6.3 6.8a8 8 0 1 0 11.4 0"/></svg>';
const UPDOWN = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7 9l5-5 5 5"/><path d="M7 15l5 5 5-5"/></svg>';

function el(tag, className, text) {
  const e = document.createElement(tag);
  if (className) e.className = className;
  if (text) e.textContent = text;
  return e;
}

function button(label, className, onclick, disabled) {
  const b = el('button', `btn ${className}`);
  b.type = 'button';
  b.innerHTML = label; // only the constants above and escaped text
  b.disabled = !!disabled;
  b.addEventListener('click', onclick);
  return b;
}

function escape(text) {
  return el('i', '', text).innerHTML;
}

function say(id, text, seconds = 6) {
  notes.set(id, text);
  clearTimeout(timers.get(id));
  if (seconds) timers.set(id, setTimeout(() => { notes.delete(id); render(last); }, seconds * 1000));
  render(last);
}

async function act(id, call, doing) {
  busy.add(id);
  say(id, doing, 0);
  try {
    say(id, await call());
  } catch (e) {
    say(id, String(e));
  }
  busy.delete(id);
  render(last);
}

function steps() {
  const ol = el('ol', 'steps');
  for (const html of ['On the machine, press <b>Mobile Start</b>.', 'Put a cup under the spout.']) {
    const li = el('li');
    li.append(el('span', 'n', ol.children.length + 1), el('span'));
    li.lastChild.innerHTML = html;
    ol.append(li);
  }
  return ol;
}

function coffeeControls(card, c) {
  const { id } = c;
  const menu = drinks.get(id);
  const chosen = picked.get(id);
  const working = busy.has(id);
  if (c.on && menu && menu.favourites.length + menu.all.length) {
    const grid = el('div', 'picks');
    grid.setAttribute('role', 'group');
    grid.setAttribute('aria-label', 'Favourite drinks');
    const select = (d) => () => { picked.set(id, d); render(last); };
    for (const d of menu.favourites) {
      const on = chosen?.id === d.id;
      const b = button((on ? TICK : '') + escape(d.name), 'pick', select(d));
      b.setAttribute('aria-pressed', on);
      grid.append(b);
    }
    const other = chosen && !menu.favourites.some((d) => d.id === chosen.id);
    const more = button((other ? TICK + escape(chosen.name) : 'Other drink') + UPDOWN, 'pick more', () => {
      menuFor = id;
      tauri?.core.invoke('other_drinks', { drinks: menu.all });
    });
    more.setAttribute('aria-haspopup', 'menu');
    if (other) more.setAttribute('aria-pressed', true);
    grid.append(more);
    card.append(grid);
  }
  if (notes.has(id)) {
    const note = el('div', 'sub', notes.get(id));
    note.setAttribute('role', 'status');
    card.append(note);
  }
  const row = el('div', 'row');
  row.append(button(POWER + (c.on ? 'Turn off' : 'Turn on'), '', () =>
    act(id, () => tauri.core.invoke('power', { deviceId: id, on: !c.on }), c.on ? 'Turning off…' : 'Turning on…'), working));
  if (c.on) {
    // Brew only for a drink picked by hand, with Mobile Start on, one request at a time.
    row.append(el('span', 'grow'), button(chosen ? `Brew ${escape(chosen.name)}` : 'Brew', 'primary', () =>
      act(id, async () => {
        const note = await tauri.core.invoke('brew', { deviceId: id, programId: chosen.id, drink: chosen.name });
        picked.delete(id);
        return note;
      }, `Starting ${chosen.name}…`), !c.mobile_start || !chosen || working));
  }
  card.append(row);
}

function render(cards) {
  last = cards;
  for (const id of drinks.keys()) {
    if (!cards.some((c) => c.coffee?.id === id && c.coffee.on)) { drinks.delete(id); picked.delete(id); }
  }
  for (const { coffee } of cards) {
    if (coffee?.on && !drinks.has(coffee.id)) {
      drinks.set(coffee.id, null);
      tauri?.core.invoke('drinks', { deviceId: coffee.id })
        .catch((e) => { say(coffee.id, String(e)); return { favourites: [], all: [] }; })
        .then((d) => { drinks.set(coffee.id, d); render(last); });
    }
  }
  list.replaceChildren(...cards.map((c) => {
    const card = el('article', 'card');
    const meta = el('div', 'meta');
    meta.append(el('span', 'label', c.label), el('span', `chip ${c.tone}`, c.chip));
    const text = el('div');
    text.append(el('div', 'title', c.title));
    if (c.coffee?.on && !c.coffee.mobile_start) text.append(steps());
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
    if (c.coffee) coffeeControls(card, c.coffee);
    return card;
  }));
}

if (tauri) {
  document.getElementById('more').addEventListener('click', () => tauri.core.invoke('more'));
  // The window is as tall as the panel.
  new ResizeObserver(() => tauri.core.invoke('fit', { height: document.querySelector('.panel').offsetHeight }))
    .observe(document.querySelector('.panel'));
  tauri.event.listen('cards', (e) => render(e.payload));
  tauri.event.listen('drink', (e) => {
    const d = drinks.get(menuFor)?.all.find((d) => String(d.id) === e.payload);
    if (d) { picked.set(menuFor, d); render(last); }
  });
  tauri.core.invoke('cards').then(render);
}
