# Sunset pond

The simple pond (`before.json`, painted as `before.png`: flat fills, gradients, a scatter of
ripples, a symbol duck) repainted with the phase 3 tools. `script.json` paints a 1920 x 1080
still (frame 0) and a 48-frame loop at 12 fps (`loop.gif`, at a quarter size): the water and
clouds drift, the duck bobs and turns a little, the reeds sway. `process.png` is the
`--snapshots --sheet` render: one picture per stage.

## What each tool did

- **Sky:** a three-stop gradient, then clouds as a `noise` fill with stops that are transparent
  for most values and warm cloud colours at the top end, inside a group with a `blur` filter so
  the edges are soft. The noise `offset` has keyframes: the clouds drift.
- **Sun:** a `screen` layer with a big radial glow, then the sun itself in a group with a `glow`
  filter, so the halo comes from the sun's own light pixels.
- **Hills:** `noise`-filled shapes; the far ones take a `filter` with `blur` and less saturation
  (haze), and a `"clip": true` `screen` layer lifts their base towards the sky colour. The near
  hills get a clipped `screen` gradient from the sun's side.
- **Water:** a `noise` fill drifting with keyframes, a clipped `multiply` gradient for depth, the
  sun's reflection as a blurred group `mask`ed to the water layer, bristle-tip ripple strokes and
  `sparkle` tips stamped down the reflection for glints.
- **Parts from the library:** `duck`, `lilypad` and `reed`, each placed with `use`; the duck gets
  a `shadow` filter and a mirrored, blurred, masked reflection; the reeds get a `lean` per copy
  from `each` and sway with a keyframed group.
- **Bank:** a jittered polygon with the `grass` tip stamped along its edge.
- **Finish:** a `"from": "below"` layer with a warm `gradient_map` at 0.22, a radial vignette on
  `multiply`, and `{"texture": "paper"}` on `multiply` at 0.18.

## Honest notes

- The sun's reflection reads as a soft pillar rather than broken light; a row of stamped
  highlights following the ripples would be truer.
- The duck's reflection is a plain mirror, blurred: real water would break it up.
- The far-hills haze is a visible band at their top edge; a noise mask would be more natural.
- The long ripple strokes are a bit regular; a hand would vary their length more.
- The whole still paints in about 9 s (with snapshots); the 48 preview frames and the GIF in
  about 16 s; the full-size loop would be about 7 minutes of painting.
