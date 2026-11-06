# Library: parts to include

Reusable parts made with Art App's tools, each a `define` with parameters. A script includes
one with a path relative to itself, then uses it like any symbol:

```json
{ "cmd": "include", "file": "../skills/art-app/library/cloud.json" }
{ "cmd": "use", "name": "cloud", "translate": [200, 80], "with": { "puff": 1.3 } }
```

`include` reads the file's `define`s and palette (not its commands). Every part is drawn around
`[0, 0]` (standing on `y = 0` where it stands), so `translate` says where it goes; the use's
`scale`, `rotate` and `mirror` place it; `with` sets its parameters; `colors` swaps its colours.
`<part>-example.json` next to each part is a small script using it; `<part>.png` is that script
painted. `pond-example.json` uses every part at once.

| Part | File | Parameters (defaults) | Notes |
|---|---|---|---|
| cloud | `cloud.json` | `puff` 1, `color` cloud, `shade` cloud_shade | About 160 wide at puff 1; soft edges. |
| water | `water.json` | `dark`, `light`, `seed` 1 | 400 x 120, its top at y = -60; noise, ripples, glints. Scale the use to fit. |
| grass | `grass.json` | `blade` [16, 30], `spacing` 5, `color1`, `color2`, `seed` 1 | 300 wide, blades standing on y = 0. |
| reed | `reed.json` | `lean` 6, `stem`, `head` | One cattail, 120 tall. `repeat` with `each` leans for a clump. |
| tree | `tree.json` | `trunk`, `leaf`, `leaf_light`, `seed` 1 | 140 wide, 170 tall, standing on y = 0; noise leaves with one outline. |
| lilypad | `lilypad.json` | `color`, `light`, `notch` 20 | 90 wide; `notch` turns the cut (degrees). |
| duck | `duck.json` | `color`, `color2`, `beak`, `ink` | Facing right, 90 wide; one outline around the whole bird. `"mirror": "x"` to face left. |

Palette names each part brings (`cloud`, `water_dark`, `grass_light`, ...) are listed in its file.
A script that defines the same colour differently, or the same symbol, is told so.
