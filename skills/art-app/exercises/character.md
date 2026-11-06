# One character in three poses

## Do

Define one small character as a symbol with parameters for its legs and arms, draw it once,
and `use` it three times: standing, walking (legs apart, arms swinging), and waving (mirrored,
one arm up). One outline around the whole figure, not part by part. 660 x 260. Label the poses.

The point: draw once, place many times; the pose is in the parameters, the shapes never
change, so the character stays the same character.

## What a good result looks like

`character.png` is one answer. `art-cli critique character.json` says:

- `readability at 96 px: 3 of 3 parts still read`: each pose reads as a silhouette.
- `out of key: nothing; the key is body (red)`.
- `focal point: the eye goes to waving; 218 px from the intent's focal point`: the intent
  names the walking pose, but the raised arm of the waving one draws the eye. That is the
  honest finding; for a sprite sheet it doesn't matter, for a picture it would.
- Expected: `flat` inside each figure (flat fills by design); `bottom-heavy` (the figures
  stand on the ground along the bottom).

## Lessons it practises

10, 26 in `../lessons.md`; and the symbols section of `../SKILL.md`.
