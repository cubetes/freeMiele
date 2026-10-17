// Draws the cards the Rust side sends. No logic about Miele here.
const tauri = window.__TAURI__;
const list = document.getElementById('cards');
// Coffee controls, kept by device id so a live update (which redraws everything) loses nothing.
const drinks = new Map(); // id -> { favourites, all }; null while loading
const picked = new Map(); // id -> { id, name }
const busy = new Set(); // ids with a request in flight
const notes = new Map(); // id -> result text, shown for a few seconds
const timers = new Map();
const care = new Map(); // id -> care or TwinDos line, the last answer
const asked = new Set(); // ids asked since the panel opened: once each (Miele rate-limits)
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
      tauri?.core.invoke('other_drinks', { drinks: menu.all, event: 'drink' });
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

// The login form's state, kept here because a redraw rebuilds the inputs. The secret lives only
// until the login succeeds, then it is cleared; Rust sends it to the Keychain and nowhere else.
const form = { id: '', secret: '', phase: 'form', error: '' }; // phase: form, waiting or connecting

async function logIn(e) {
  e.preventDefault();
  form.phase = 'waiting';
  form.error = '';
  render(last);
  try {
    await tauri.core.invoke('log_in', { clientId: form.id, clientSecret: form.secret });
    form.id = form.secret = '';
    form.phase = 'connecting';
  } catch (err) {
    form.phase = 'form';
    form.error = String(err) === 'Login cancelled.' ? '' : String(err);
  }
  render(last);
}

function loginForm(card) {
  const f = el('form', 'form');
  f.noValidate = true;
  f.addEventListener('submit', logIn);
  if (form.phase !== 'form') {
    const wait = form.phase === 'waiting';
    const note = el('div', 'sub', wait ? 'Waiting for Miele sign-in in your browser…' : 'Logged in. Loading your appliances…');
    note.setAttribute('role', 'status');
    f.append(note);
    if (wait) f.append(button('Cancel', '', () => tauri.core.invoke('cancel_login')));
    card.append(f);
    return;
  }
  f.append(el('div', 'sub', 'Enter the keys you got from Miele. They are kept in your Mac’s Keychain.'));
  for (const [key, label, type] of [['id', 'Client ID', 'text'], ['secret', 'Client secret', 'password']]) {
    const input = el('input');
    input.type = type;
    input.value = form[key];
    input.autocomplete = 'off';
    input.spellcheck = false;
    input.addEventListener('input', () => { form[key] = input.value; });
    const field = el('label', 'field');
    field.append(label, input);
    f.append(field);
  }
  if (form.error) {
    const err = el('div', 'sub err', form.error);
    err.setAttribute('role', 'alert');
    f.append(err);
  }
  const log = button('Log in with Miele', 'primary', () => {});
  log.type = 'submit';
  const help = el('button', 'link', 'How to get your keys');
  help.type = 'button';
  help.addEventListener('click', () => tauri.core.invoke('open_keys_help'));
  const row = el('div', 'row');
  row.append(log, el('span', 'grow'), help);
  f.append(row);
  card.append(f);
}

function render(cards) {
  last = cards;
  if (!cards.some((c) => c.login)) form.phase = 'form';
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
  for (const { id } of cards) {
    if (id && !asked.has(id)) {
      asked.add(id);
      tauri?.core.invoke('care', { deviceId: id }).then((t) => { care.set(id, t); render(last); }, () => {});
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
    if (c.minutes_left != null) {
      const at = new Date(Date.now() + c.minutes_left * 60000);
      text.append(el('div', 'sub', `done at ${at.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`));
    }
    card.append(meta, text);
    if (c.login) loginForm(card);
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
    if (care.get(c.id)) card.append(el('div', /empty/.test(care.get(c.id)) ? 'sub care low' : 'sub care', care.get(c.id)));
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
  // Favourites changed in Settings: fetch the drinks again.
  tauri.event.listen('settings', () => { drinks.clear(); render(last); });
  // Ask again each time the panel is opened, not on every live update.
  // Ask again each time the panel opens; keep showing the old line until the new one arrives.
  document.addEventListener('visibilitychange', () => {
    if (!document.hidden) { asked.clear(); render(last); }
  });
  tauri.core.invoke('cards').then(render);
}
