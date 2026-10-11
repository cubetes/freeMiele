# Supported appliances

What freeMiele can do depends on the model: Miele's API allows different things per appliance.
This table lists models confirmed on real machines. Own one that's missing? Open an
[Appliance support report](https://github.com/cubetes/freeMiele/issues/new?template=appliance.yml).

| Model | Type | Status | Power on/off | Start | Extras | Notes |
|---|---|---|---|---|---|---|
| CM 6160 | Coffee machine | Yes | Yes (one at a time, depending on current state) | Yes: 21 drinks via `brew` | Light | Needs **Mobile Start** turned on at the machine before each drink |
| WTR860 | Washer-dryer | Not yet: shows "Not connected" | No (not offered by the API) | Planned: start, stop, delayed start ([v0.5 Boron](https://github.com/cubetes/freeMiele/milestone/4)) | n/a | Status details planned in [#6](https://github.com/cubetes/freeMiele/issues/6) |

Last checked: 11 Oct 2026.
