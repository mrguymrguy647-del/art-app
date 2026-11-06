# A colour study in one key, five colours

## Do

Paint a small still life (a cup on a table by a window) with a palette of exactly five
colours, all on one side of the colour wheel: a shadow, a deep, a mid, a light and a cream.
Make the picture by value, not by hue: the lights go on the rim of the cup and the window,
the darks under the cup. 640 x 360. Name the three main colours in the intent's `key_colors`.

## What a good result looks like

`colour-study.png` is one answer. `art-cli critique colour-study.json` says:

- `colour: 2 hues carry 90% of the colour; warm 100%`: one key, as asked.
- `out of key: nothing; the key is mid (orange), light (orange), deep (orange)`.
- `values: 4 value groups when squinted (dark 61%, mid 17%, light 13%, white 10%)`: the darks
  dominate and the lights are small: the rim and the window.
- `focal contrast: the focal point has the strongest contrast in the picture (1.5x the next
  strongest, at window)`: the cup's cream rim on the deep coffee beats the window.
- Expected: `table is flat` (a flat fill, as in a study); `table merges with its surroundings
  at thumbnail size` (the table and the wall are close in value: the one thing to improve).

## Lessons it practises

11, 12, 14 in `../lessons.md`.
