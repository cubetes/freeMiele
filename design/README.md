# freeMiele design system

The source of truth for freeMiele's look. `tokens.json` holds every colour (light and dark), font, spacing step and corner radius; apps read it directly. Logo files are in [`../assets/logo/`](../assets/logo/).

freeMiele is an open-source, community-built way to control Miele appliances from your own computer. Its identity has to do two things at once: feel at home next to a premium kitchen appliance, and never be mistaken for Miele itself.

## Identity rules

- Write the name as **freeMiele**: lower-case `free`, capital `M`, one word. Set it in `display` weight 700, `ink` or `brand`.
- Always say "unofficial" near the name on first use (README, app About screen, website footer). Never use Miele's logo, wordmark, red or typeface.
- `brand` (teal) is the face of the project. `crema` is the one warm accent. Red appears only as `fault`, and only small.

## Logo: the Dial

The mark is a `brand` tile (corner radius 28 on its 120 grid, about a quarter of the side) holding a `crema` knob with a pointer, and a half-circle scale that ends where the pointer points: an appliance dial turned to a setting. It works for every appliance with a dial, not only coffee.

- Use `freemiele-mark.svg` on light grounds and `freemiele-mark-dark.svg` on dark ones. The cut-out scale and pointer take the ground colour, so pick the file that matches the ground.
- At 32px and below (favicons, menu bar, tabs) use `freemiele-mark-small.svg`: heavier scale and pointer, so they survive.
- Lockup: mark at 1.2× the cap height of the name, then **freeMiele** in `display` 700, gap `space-2` to `space-4`. Keep clear space of a quarter of the mark's width on every side.
- Don't recolour, rotate, outline, add shadows, or put the mark on a busy photo. Don't turn the pointer to another angle: it always points to 1:30.

## Colour: how the palette was derived

The starting point is Miele's brand red, approximately `#8C0014` (hue 351°, very dark and saturated). freeMiele takes its colours from that hue's relationships on the colour wheel, then keeps away from the red itself so the two are never confused.

| Harmony | Hues from 351° | What it would give | Used? |
| --- | --- | --- | --- |
| Complementary | 171° | Deep teal: calm, clean, "water and steam" | **Yes: `brand`** |
| Analogous | 21° and 321° | Copper-orange and magenta | **Copper only: `crema`** |
| Split-complementary | 141° and 201° | Green plus cyan-blue | No: two cool hues compete with teal |
| Triadic | 111° and 231° | Grass green plus royal blue | No: too playful for appliances |

- **Complementary teal** (`brand`, hue 174°) gives the strongest visual separation from Miele red while still being its natural partner. Use it for primary actions, links and the "Ready" state.
- **Analogous copper** (`crema`, hue 23°) keeps a family tie to the red, and is the colour of espresso crema. Use it for brewing or running progress and single highlights, never for large backgrounds.
- **Neutrals** are biased toward teal: `surface` is a lotus white (a nod to the "blanco loto" finish), `ink` is a teal-black. Never use a pure grey.
- Dark theme lightens `brand` and `crema`, so their `on-brand` and `on-crema` text turns dark. Never put literal white on them.

## Colour in use

- Body copy: `ink` on `surface` or `surface-raised`. Secondary text: `ink-muted`.
- Primary button: `brand` fill, `on-brand` label, `radius-sm`. Secondary button: `surface-raised` fill, `line` border plus `ink` label.
- Appliance states always pair a colour with a word, so colour-blind users lose nothing:
  - Ready → `brand` · Running / brewing → `crema` · Needs attention, Mobile start off → `attention` · Fault → `fault` · Off, Not connected → `ink-muted`.
- Focus: a 2px solid `focus` ring with 2px offset on every interactive element.

## Type

- `display` family (Schibsted Grotesk) for the `display`, `heading` and `title` styles: sturdy, slightly engraved, like a control panel label.
- `body` family (Atkinson Hyperlegible Next) for `body`, `small` and `label`. Chosen for legibility: the project should be easy to read for everyone.
- `mono` family (JetBrains Mono) for `code`. freeMiele starts in the terminal, so commands are first-class content: show them in `code` on `surface-raised` with `radius-sm`.
- Fonts are free Google Fonts. Load them from Google Fonts; the fallbacks in each family are safe.

## Writing

- Short sentences, plain words, the user's language. "Turn on Mobile Start at the machine, then run this again." Not "Remote enablement precondition failed."
- Name things the way the machine and its manual do: drink names exactly as the appliance lists them ("Flat white", "Caffè latte"), states as the appliance reports them ("Off", "Not connected").
- Errors say what happened and what to do next. No apologies, no blame.
- No emoji in the interface.

## Spacing, shape and layout

- Spacing steps: `space-1` 4px, `space-2` 8px, `space-4` 16px, `space-8` 32px. Use `space-4` for card padding and phone gutters, `space-8` between sections.
- Corners: `radius-sm` for anything you press (crisp, like touch keys), `radius-md` for cards, `radius-pill` for status chips only.
- Borders before shadows: separate panels with `line`, not drop shadows.
- Layout: one appliance per card, its state chip top-right, its main action bottom-right.

## Iconography

No icon set chosen yet. Until one is, use words, not pictures. When one is added, use a single-weight outline set at 20px, drawn in `ink` or the state colour.
