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

// Icons: 20 px grid, 1.5 stroke, drawn for freeMiele. Rust picks the key (`icon` on the card).
const SMALL = '<path d="M6 10h7v2.5a3.5 3.5 0 0 1-7 0z"/><path d="M13 11h.8a1.5 1.5 0 0 1 0 3h-1.3"/><path d="M4 17.5h11"/>';
const MUG = '<path d="M4.5 8h9v5a4 4 0 0 1-4 4h-1a4 4 0 0 1-4-4z"/><path d="M13.5 9h1.2a2 2 0 0 1 0 4h-1.4"/>';
const TEACUP = '<path d="M4 10h10v1.5a4.5 4.5 0 0 1-4.5 4.5h-1A4.5 4.5 0 0 1 4 11.5z"/><path d="M14 11h.8a1.6 1.6 0 0 1 0 3.2h-1.2"/><path d="M3 18h12"/>';
const JUG = '<path d="M7 6h7v9.5a1.5 1.5 0 0 1-1.5 1.5h-4A1.5 1.5 0 0 1 7 15.5z"/><path d="M7 6L5 4.8"/><path d="M14 8h.8a1.6 1.6 0 0 1 1.6 1.6v2.3a1.6 1.6 0 0 1-1.6 1.6H14"/>';
const GLASS = '<path d="M5.5 6h9l-.9 11H6.4z"/><path d="M10 9c1.4 1.7 2 2.6 2 3.5a2 2 0 0 1-4 0c0-.9.6-1.8 2-3.5z"/>';
const STEAM = (y) => `<path d="M8 ${y}c-.7.8.7 1.6 0 2.5M11.5 ${y}c-.7.8.7 1.6 0 2.5"/>`;
const PETAL = 'M9 5c-1.3-.8-1.5-2.3 0-3.2c1.5.9 1.3 2.4 0 3.2z';
const WASHER = '<path d="M4.5 2.5h11a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-13a1 1 0 0 1 1-1z"/><path d="M3.5 6h13"/><path d="M6 4.25h.01M8 4.25h.01"/><circle cx="10" cy="11.5" r="3.8"/>';
const DISH = '<path d="M4.5 2.5h11a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-13a1 1 0 0 1 1-1z"/><path d="M3.5 6h13"/><path d="M12 4.25h2"/><path d="M6 13.5h8M6.5 13.5v-2.5M10 13.5v-3M13.5 13.5v-2.5"/>';
const I = {
  ristretto: SMALL + '<path d="M7.5 13.5h4"/><path d="M9.5 4.5v3"/>',
  espresso: SMALL + STEAM(4.5),
  espresso_macchiato: SMALL + '<path d="M8 10a1.5 1.3 0 0 1 3 0"/>',
  cappuccino_italiano: SMALL + '<path d="M6 10a3.5 3 0 0 1 7 0"/>',
  coffee: MUG + STEAM(2.5),
  cappuccino: MUG + '<path d="M4.5 8a4.5 3.5 0 0 1 9 0"/>',
  long_coffee: '<path d="M5 4h8v9.5a3.5 3.5 0 0 1-3.5 3.5h-1A3.5 3.5 0 0 1 5 13.5z"/><path d="M13 6h1.2a2 2 0 0 1 0 4H13"/><path d="M5 7h8"/>',
  flat_white: '<path d="M3.5 10h11v1.5a4.5 4.5 0 0 1-4.5 4.5H8a4.5 4.5 0 0 1-4.5-4.5z"/><path d="M14.5 11h1a1.5 1.5 0 0 1 0 3H14"/><path d="M2.5 18h14"/><path d="M5.5 12h7"/>',
  cafe_au_lait: '<path d="M3 8.5h14a7 7 0 0 1-14 0z"/><path d="M7 17.5h6"/><path d="M6.5 11.5c1.5-1 5.5 1 7 0"/>',
  caffe_latte: '<path d="M5 3h8v13a1.5 1.5 0 0 1-1.5 1.5h-5A1.5 1.5 0 0 1 5 16z"/><path d="M13 6.5h1.2a1.8 1.8 0 0 1 1.8 1.8v3.4a1.8 1.8 0 0 1-1.8 1.8H13"/><path d="M5 9h8"/>',
  latte_macchiato: '<path d="M5.5 2.5h9l-1.3 14a1 1 0 0 1-1 .9H7.8a1 1 0 0 1-1-.9z"/><path d="M6 7.5h8"/><path d="M6.5 12.5h7"/>',
  very_hot_water: GLASS + STEAM(1.5),
  hot_water: GLASS,
  hot_milk: JUG + '<path d="M9.5 1.5c-.6.7.6 1.3 0 2M12 1.5c-.6.7.6 1.3 0 2"/>',
  milk_foam: JUG + '<path d="M8 6a1.6 1.6 0 0 1 1.5-2.4a2 2 0 0 1 3.6.6A1.5 1.5 0 0 1 13.5 6"/>',
  black_tea: TEACUP + '<path d="M9 10V6.5"/><path d="M7.5 3.5h3v3h-3z"/>',
  herbal_tea: TEACUP + '<path d="M9 9.5V3"/><path d="M9 6c-1.6 0-2.6-1-2.6-2.6C8 3.4 9 4.4 9 6z"/><path d="M9 7.5c1.6 0 2.6-1 2.6-2.6C10 4.9 9 5.9 9 7.5z"/>',
  fruit_tea: TEACUP + '<circle cx="7.5" cy="7" r="1.6"/><circle cx="11" cy="7.5" r="1.6"/><path d="M7.5 5.4C8 3.6 9 2.8 10.5 2.5M11 5.9c0-1.4-.2-2.4-.5-3.4"/>',
  green_tea: TEACUP + '<path d="M6 8c0-3.2 2.4-5.5 6.5-5.5c0 4-2.4 5.5-6.5 5.5z"/><path d="M6 8l3.5-3.5"/>',
  white_tea: TEACUP + [0, 90, 180, 270].map((a) => `<path d="${PETAL}" transform="rotate(${a} 9 5)"/>`).join(''),
  japanese_tea: '<path d="M5.5 8h9l-.8 7.5a1.6 1.6 0 0 1-1.6 1.5H7.9a1.6 1.6 0 0 1-1.6-1.5z"/><path d="M5 18.5h10"/><path d="M8 6c0-2.2 1.6-3.8 4.5-3.8c0 2.8-1.6 3.8-4.5 3.8z"/>',
  // appliances and states
  machine: '<path d="M5 2.5h10a1 1 0 0 1 1 1v14H4v-14a1 1 0 0 1 1-1z"/><path d="M4 6h12"/><path d="M8.5 8.5h3v2h-3z"/><path d="M8.5 14h3v2h-3z"/>',
  washer: WASHER,
  dishwasher: DISH,
  dishwashing: DISH + '<path class="drop" d="M8 7.8v1.4"/><path class="drop d2" d="M10 7.8v1.4"/><path class="drop d3" d="M12 7.8v1.4"/>',
  fridge: '<path d="M5.5 1.5h9a1 1 0 0 1 1 1v15a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1v-15a1 1 0 0 1 1-1z"/><path d="M4.5 8h11"/><path d="M7 4v2M7 10.5v3"/>',
  power: '<path d="M10 3v6.5"/><path d="M5.6 6a6 6 0 1 0 8.8 0"/>',
  brewing: '<path d="M5.5 9.5h8v3a4 4 0 0 1-8 0z"/><path d="M13.5 10.5h.8a1.6 1.6 0 0 1 0 3.2h-1.2"/><path d="M3.5 18h12"/><path class="drop" d="M8.5 3v1.5"/><path class="drop d2" d="M10.5 3v1.5"/>',
  rinsing: '<path d="M7 2.5h6V5H7z"/><path class="drop" d="M8 8v1.5"/><path class="drop d2" d="M10 8v1.5"/><path class="drop d3" d="M12 8v1.5"/><path d="M3.5 14.5a6.5 3 0 0 0 13 0"/>',
  washing: WASHER + '<path class="drum" d="M7.4 12c.9-.7 1.7-.7 2.6 0s1.7.7 2.6 0"/>',
  drying: WASHER + '<path class="heat" d="M8.7 13.6c-.6-.7.6-1.4 0-2.1s.6-1.4 0-2.1"/><path class="heat d2" d="M11.3 13.6c-.6-.7.6-1.4 0-2.1s.6-1.4 0-2.1"/>',
  finished: '<circle cx="10" cy="10" r="7.5"/><path class="tick" pathLength="20" d="M6.6 10.2l2.4 2.4 4.5-4.7"/>',
  attention: '<g class="pulse"><path d="M10 3l7.5 13h-15z"/><path d="M10 8v3.5M10 14h.01"/></g>',
  offline: '<path d="M3 7.5a10 10 0 0 1 14 0M5.5 10.5a6.5 6.5 0 0 1 9 0M8 13.5a3 3 0 0 1 4 0M10 16.5h.01"/><path d="M3.5 3.5l13 13"/>',
};
const icon = (k) => `<svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${I[k] || I.machine}</svg>`;

// The loops start at a point set by the clock, so a redraw (which rebuilds everything) never jumps.
const phase = (ms) => `-${Date.now() % ms}ms`;
let openedAt = null; // when the panel last opened with cards; the arc grows only then
const prev = new Map(); // card key -> chip word at the last draw
const changedAt = new Map(); // card key -> when its chip word last changed
// Two identical appliances share a label, so the key adds the card's place in the (sorted) list.
const key = (c, i) => `${i}:${c.label}`;
const said = document.getElementById('said');
const TONE = { ready: 'brand', running: 'crema', attention: 'attention', fault: 'fault', off: 'muted' };

// The ring: r=19 on a 44 box, pathLength 100, starting at 12 o'clock.
function ring(c) {
  const circle = (cls, extra = '') => `<circle class="rg ${cls}" cx="22" cy="22" r="19" pathLength="100" ${extra}/>`;
  const running = c.tone === 'running';
  let body;
  if (c.icon === 'offline') body = circle('m thin', 'stroke-dasharray="0.01 6.25"');
  else if (c.tone === 'off') body = circle('t thin');
  else if (c.tone === 'attention') body = circle('a', 'stroke-dasharray="88 12" stroke-dashoffset="-6"');
  else if (c.tone === 'fault') body = circle('f', 'stroke-dasharray="88 12" stroke-dashoffset="-6"');
  else if (!running) body = circle(c.icon === 'finished' ? 'b' : 'b thin');
  else if (c.progress != null) {
    const age = openedAt === null ? 9e9 : Date.now() - openedAt;
    const grow = age < 1400 ? ` grow" style="animation-delay:-${age}ms` : '';
    body = circle('t') + circle(`c${grow}`, `stroke-dasharray="${c.progress} 100"`);
  } else body = circle('t') + `<g class="spin" style="animation-delay:${phase(1600)}">${circle('c', 'stroke-dasharray="25 75"')}</g>`;
  const r = el('div', 'ring');
  r.innerHTML = `<svg class="r" width="44" height="44" viewBox="0 0 44 44" aria-hidden="true"><g transform="rotate(-90 22 22)">${body}</g></svg><span class="ico" style="color:var(--${TONE[c.tone]})">${icon(c.icon)}</span>`;
  r.style.setProperty('--d16', phase(1600));
  r.style.setProperty('--d24', phase(2400));
  r.style.setProperty('--d40', phase(4000));
  if (running && c.progress != null) {
    const m = c.minutes_left;
    r.setAttribute('role', 'progressbar');
    r.setAttribute('aria-label', c.label);
    r.setAttribute('aria-valuemin', 0);
    r.setAttribute('aria-valuemax', 100);
    r.setAttribute('aria-valuenow', c.progress);
    r.setAttribute('aria-valuetext', `${c.progress} percent done${m != null ? `, ${m} ${m === 1 ? 'minute' : 'minutes'} left` : ''}`);
  } else if (running) {
    r.setAttribute('role', 'progressbar');
    r.setAttribute('aria-label', `${c.chip}, ${c.title}`);
  } else r.setAttribute('aria-hidden', 'true');
  return r;
}

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
  if (openedAt === null && cards.length) openedAt = Date.now();
  // Motion only when a chip word changes; one polite announcement for those changes, nothing else.
  const spoken = [];
  cards.forEach((c, i) => {
    const k = key(c, i);
    if (prev.has(k) && prev.get(k) !== c.chip) {
      changedAt.set(k, Date.now());
      spoken.push(`${c.label}: ${c.chip}`);
    }
    prev.set(k, c.chip);
  });
  if (spoken.length) said.textContent = spoken.join('. ');
  list.replaceChildren(...cards.map((c, i) => {
    const since = Date.now() - (changedAt.get(key(c, i)) ?? 0);
    const card = el('article', ['card', c.tone === 'attention' && 'attention', c.icon === 'finished' && 'finished', since < 2500 && 'chg'].filter(Boolean).join(' '));
    card.style.setProperty('--age', `${since}ms`); // a redraw mid-motion carries on, it does not restart
    const meta = el('div', 'meta');
    meta.append(el('span', 'label', c.label), el('span', `chip ${c.tone}`, c.chip));
    const text = el('div', 'body');
    text.append(meta, el('div', 'title', c.title));
    if (c.coffee?.on && !c.coffee.mobile_start) text.append(steps());
    if (c.sub) text.append(el('div', 'sub', c.sub));
    if (c.minutes_left != null) {
      const at = new Date(Date.now() + c.minutes_left * 60000);
      text.append(el('div', 'sub', `done at ${at.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`));
    }
    card.append(ring(c), text);
    if (c.login) loginForm(card);
    if (c.coffee) coffeeControls(card, c.coffee);
    if (care.get(c.id)) card.append(el('div', /empty/.test(care.get(c.id)) ? 'sub care low' : 'sub care', care.get(c.id)));
    return card;
  }));
  fit();
}

// Ask for the window to be as tall as everything would be; Rust caps it at the screen and the list scrolls.
function fit() {
  if (!tauri) return;
  const panel = document.querySelector('.panel');
  tauri.core.invoke('fit', { height: panel.offsetHeight - list.clientHeight + list.scrollHeight });
}

if (tauri) {
  document.getElementById('more').addEventListener('click', () => tauri.core.invoke('more'));
  // The window is as tall as the panel.
  new ResizeObserver(fit).observe(document.querySelector('.panel'));
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
    if (!document.hidden) { asked.clear(); openedAt = null; render(last); }
  });
  tauri.core.invoke('cards').then(render);
}
