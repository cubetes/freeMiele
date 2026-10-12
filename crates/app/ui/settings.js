// Draws the Settings window. The rules (five at most, no repeats, defaults) live in Rust.
const tauri = window.__TAURI__;
const $ = (id) => document.getElementById(id);
const MAX = 5;
const GRIP = '<svg width="10" height="14" viewBox="0 0 10 14" fill="currentColor" aria-hidden="true"><circle cx="2.5" cy="2.5" r="1.3"/><circle cx="7.5" cy="2.5" r="1.3"/><circle cx="2.5" cy="7" r="1.3"/><circle cx="7.5" cy="7" r="1.3"/><circle cx="2.5" cy="11.5" r="1.3"/><circle cx="7.5" cy="11.5" r="1.3"/></svg>';
const MINUS = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M8 12h8"/></svg>';
let s = { favourites: [], time_left: true, washer_alert: true };
let drinks = []; // what the machine can make; empty while it is off
let sel = 0; // selected row
let dragging = -1;

function el(tag, className, html) {
  const e = document.createElement(tag);
  if (className) e.className = className;
  if (html) e.innerHTML = html;
  return e;
}

async function save(next) {
  s = await tauri.core.invoke('save_settings', { settings: next });
  render();
}

function move(from, to) {
  if (from === to || to < 0 || to >= s.favourites.length) return;
  const favourites = [...s.favourites];
  favourites.splice(to, 0, ...favourites.splice(from, 1));
  sel = to;
  save({ ...s, favourites });
}

function row(name, i) {
  const li = el('li', 'srow');
  li.setAttribute('role', 'option');
  li.setAttribute('aria-label', `${name}, ${i + 1} of ${s.favourites.length}`);
  li.setAttribute('aria-selected', i === sel);
  li.tabIndex = i === sel ? 0 : -1;
  li.draggable = true;
  const grip = el('span', 'grip', GRIP);
  grip.setAttribute('aria-hidden', 'true');
  const number = el('span', 'num-i');
  number.setAttribute('aria-hidden', 'true');
  number.textContent = i + 1;
  const label = el('span', 'grow');
  label.textContent = name;
  const remove = el('button', 'icon-btn sm', MINUS);
  remove.type = 'button';
  remove.setAttribute('aria-label', `Remove ${name}`);
  remove.addEventListener('click', (e) => {
    e.stopPropagation();
    sel = Math.max(0, Math.min(sel, s.favourites.length - 2));
    save({ ...s, favourites: s.favourites.filter((_, j) => j !== i) });
  });
  li.append(grip, number, label, remove);
  li.addEventListener('click', () => { sel = i; render(); $('favs').children[i].focus(); });
  li.addEventListener('dragstart', () => { dragging = i; });
  li.addEventListener('dragover', (e) => { e.preventDefault(); li.classList.add('over'); });
  li.addEventListener('dragleave', () => li.classList.remove('over'));
  li.addEventListener('drop', (e) => { e.preventDefault(); if (dragging >= 0) move(dragging, i); dragging = -1; });
  return li;
}

function render() {
  const list = $('favs');
  const hadFocus = list.contains(document.activeElement);
  list.replaceChildren(...s.favourites.map(row));
  if (hadFocus) list.children[sel]?.focus();
  const n = s.favourites.length;
  const choices = drinks.filter((d) => !s.favourites.some((f) => f.toLowerCase() === d.name.toLowerCase()));
  $('add').disabled = n >= MAX || !choices.length;
  $('count').textContent = n >= MAX ? `${MAX} of ${MAX}. Remove one to add another.`
    : choices.length ? `${n} of ${MAX}` : `${n} of ${MAX}. Turn the coffee machine on to add drinks.`;
  $('s-time').checked = s.time_left;
  $('s-notify').checked = s.washer_alert;
}

$('favs').addEventListener('keydown', (e) => {
  const d = { ArrowUp: -1, ArrowDown: 1 }[e.key];
  if (!d || e.target !== e.currentTarget.children[sel]) return;
  e.preventDefault();
  const to = sel + d;
  if (to < 0 || to >= s.favourites.length) return;
  if (e.altKey && e.metaKey) return move(sel, to);
  sel = to;
  render();
  $('favs').children[sel].focus();
});
$('add').addEventListener('click', () => {
  const have = s.favourites.map((f) => f.toLowerCase());
  tauri.core.invoke('other_drinks', { drinks: drinks.filter((d) => !have.includes(d.name.toLowerCase())), event: 'fav' });
});
$('restore').addEventListener('click', async () => { s = await tauri.core.invoke('restore_favourites'); sel = 0; render(); });
$('s-time').addEventListener('change', (e) => save({ ...s, time_left: e.target.checked }));
$('s-notify').addEventListener('change', (e) => save({ ...s, washer_alert: e.target.checked }));
$('logout').addEventListener('click', async () => {
  try {
    await tauri.core.invoke('log_out');
    $('acc-title').textContent = 'Logged out of your Miele account';
    $('acc-sub').textContent = 'Run `miele login` in Terminal to log in again.';
    $('logout').hidden = true;
  } catch (e) {
    $('acc-sub').textContent = String(e);
  }
});

if (tauri) {
  tauri.event.listen('fav', (e) => {
    const d = drinks.find((d) => String(d.id) === e.payload);
    if (d) { sel = s.favourites.length; save({ ...s, favourites: [...s.favourites, d.name] }); }
  });
  tauri.event.listen('settings', (e) => { s = e.payload; sel = Math.min(sel, Math.max(0, s.favourites.length - 1)); render(); });
  tauri.core.invoke('settings').then((v) => { s = v; render(); });
  tauri.core.invoke('machine').then((m) => {
    drinks = m.drinks;
    $('acc-sub').textContent = m.appliances;
    render();
  }).catch((e) => {
    $('acc-title').textContent = "Can't read your Miele account";
    $('acc-sub').textContent = String(e);
  });
}
