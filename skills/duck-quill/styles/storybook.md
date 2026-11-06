# Storybook

Flat local colours, one clean outline, simple shapes: the look of a children's book or a
print. What the critique calls "flat" is the style here; the silhouettes do the work.

**Palette rules.** 6 to 10 colours. Two or three hues plus their lights and darks; one accent.
Saturation 0.4 to 0.8 for the main shapes, lower for the background. Values in three or four
clear steps, no gradients inside a shape bigger than a highlight.

**Brush and fill settings that fit.** Plain fills (`fill`, `outline`, `outline_size` 3 to 6 at
1080p); `"outline": {"color": "ink", "size": 4}` on a character's `use`; `jitter` 2 to 4 for a
hand-cut edge; `polygon` and `path` over `ellipse` for anything that isn't round; no noise
fills, no blur. Text in the picture is fine.

**What to avoid.** Gradients and glows (they turn it painterly); outlines part by part (seams);
more than one accent colour; tiny details (they vanish at thumbnail size, where this style
should read best).

**Pieces that show it.** `examples/two-ducks.json`, `library/duck.json`, `examples/duck-sheet.json`;
Hokusai's Great Wave in `references/` (flat colour, outlines, two hues).

**What the numbers say.** `flat over` a large share of named parts is expected; value groups
three or more; readability every part; nothing out of key.
