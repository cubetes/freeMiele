# Supported appliances

What freeMiele can do depends on the model: Miele's API allows different things per appliance.
This table lists models confirmed on real machines. Own one that's missing? Open an
[Appliance support report](https://github.com/cubetes/freeMiele/issues/new?template=appliance.yml)
and attach the file from `miele dump > appliance.json`.

| Model | Type | Status | Power on/off | Start | Extras | Notes |
|---|---|---|---|---|---|---|
| CM 6160 | Coffee machine | Yes: program and phase while working (e.g. "In use, Degreasing the brew unit") | Yes (one at a time, depending on current state) | Yes: 21 drinks via `brew` | `miele care`: lifetime care counts and care schedule; light | Needs **Mobile Start** turned on at the machine before each drink. The API doesn't say when care is due |
| WTR860 | Washer-dryer | Yes: program, phase, time left, "Finished" (e.g. "In use, Sportswear · Main wash, 0h23 left") | Off: yes, when on | Planned: start, stop, pause, delayed start ([v0.5 Boron](https://github.com/cubetes/freeMiele/milestone/4)). Stop and pause are offered while running | TwinDos levels (unit unconfirmed); last 5 runs | No program list from the API. Water and energy use stay at 0 on this model |

Last checked: 12 Oct 2026 (Lithium 0.3.0), on macOS 27 with Apple silicon.

## Not tried yet: testers wanted

Nobody has run freeMiele against these types yet. Miele's API says they're supported, but each
model allows different things, so a real report is what turns a row into "works". Run
`miele dump > appliance.json` and open an [Appliance support report](https://github.com/cubetes/freeMiele/issues/new?template=appliance.yml).

| Type | What Miele's API offers (per its docs) | freeMiele today |
|---|---|---|
| Other coffee machines | Status, power, drinks from the machine's own list, light | Should work like the CM 6160; untested |
| Washing machine | Status, program, phase, time left, start, stop, delayed start, last runs | `status` should show program, phase and time left; untested |
| Tumble dryer | As washing machine, plus drying level | As above; untested |
| Dishwasher | Status, program, phase, time left, start, stop, delayed start, salt / rinse aid / PowerDisc levels, last runs | Basic status only; untested ([#42](https://github.com/cubetes/freeMiele/issues/42)) |
| Oven, steam oven, microwave, combi | Status, program, temperatures, door, light, start, stop, power | Basic status only; untested |
| Fridge, freezer, wine unit | Temperatures per zone, Holiday / Party / Sabbath mode, SuperCool / SuperFreeze | Basic status only; untested ([#30](https://github.com/cubetes/freeMiele/issues/30)) |
| Hood | Status, fan level, light, colours, filter saturation | Basic status only; untested |
| Hob | Status, cooking zone levels (read-only) | Basic status only; untested |
| Robot vacuum | Status, battery, programs, rooms | Basic status only; untested |
