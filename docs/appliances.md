# Supported appliances

What freeMiele can do depends on the model: Miele's API allows different things per appliance.
This table lists models confirmed on real machines. Own one that's missing? Open an
[Appliance support report](https://github.com/cubetes/freeMiele/issues/new?template=appliance.yml).

| Model | Type | Status | Power on/off | Start | Extras | Notes |
|---|---|---|---|---|---|---|
| CM 6160 | Coffee machine | Yes: program and phase while working (e.g. "In use, Degreasing the brew unit") | Yes (one at a time, depending on current state) | Yes: 21 drinks via `brew` | `miele care`: lifetime care counts and care schedule; light | Needs **Mobile Start** turned on at the machine before each drink. The API doesn't say when care is due |
| WTR860 | Washer-dryer | Yes: program, phase, time left, "Finished" (e.g. "In use, Sportswear · Main wash, 0h23 left") | Off: yes, when on | Planned: start, stop, pause, delayed start ([v0.5 Boron](https://github.com/cubetes/freeMiele/milestone/4)). Stop and pause are offered while running | TwinDos levels (unit unconfirmed); last 5 runs | No program list from the API. Water and energy use stay at 0 on this model |

Last checked: 11 Oct 2026 (Helium 0.2.0).
