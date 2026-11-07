# Pixel art

Crisp whole pixels in a locked palette, drawn big and shrunk: sprites and tiles for games.

**Palette rules.** 4 to 8 colours, locked (`pixel` on the canvas: no hex colours in commands).
One dark outline colour, two or three values per material, one highlight. Keep hues few; let
value do the work.

**Settings that fit.** `{ "cmd": "canvas", "width": 32, "height": 32, "pixel": { "supersample": 8,
"outline": "ink" } }`; decimals in positions are fine; `pixels` rows for tiny things (eyes,
glints); animation moves by whole pixels; `--zoom 8` to look, `export --scale 1` for the game
and `--scale 8` for people.

**What to avoid.** Gradients, blur, noise and opacity (they turn into dither or mud);
anti-aliased edges (the supersample handles them); more colours than the palette.

**Pieces that show it.** `examples/duck-pixel.json` (a 32-pixel duck with idle and walk).

**What the numbers say.** The critique's texture rules are for painted pictures; for a sprite,
`quill check` (the motion summary) and `--pixels` (the rows of letters) matter more. The
silhouette view still applies: a sprite reads by its silhouette first.
