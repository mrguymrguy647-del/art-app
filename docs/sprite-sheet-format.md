# Sprite sheets: the PNG and its JSON

`quill export duck.json` writes `duck.png` and `duck.json` (and the app's **File → Export
animation** does the same). This page says what's in them, so another program, such as Duck
Forge, can load them.

## The picture

One PNG with a see-through background. Every frame is the same size (the script's canvas,
times `scale`). The frames sit in a grid: **one row per animation**, that animation's frames
left to right from frame 0, every cell the frame size, and shorter rows padded on the right with
empty cells. Frames are **numbered across the grid, row by row, from 0**: the frame in row `r`
and column `c` is number `r × columns + c`. Cutting the picture into cells of the frame size, in
reading order, gives exactly that numbering (`sprite.frames(w, h)` in Duck Forge).

## The JSON

```json
{
  "image": "duck.png",
  "frame": { "width": 128, "height": 128 },
  "columns": 8,
  "rows": 2,
  "scale": 1,
  "pivot": [64.0, 116.0],
  "animations": {
    "idle": { "row": 0, "frames": [0, 1, 2, 3], "fps": 6.0, "loop": true },
    "walk": { "row": 1, "frames": [8, 9, 10, 11, 12, 13, 14, 15], "fps": 12.0, "loop": true }
  }
}
```

| Field | |
|---|---|
| `image` | The sheet's file name, next to the JSON. |
| `frame` | One cell's size in pixels (`width`, `height`), after scaling. |
| `columns`, `rows` | The grid: `columns` is the longest animation's frame count; `rows` is the number of animations. The picture is `columns × width` by `rows × height`. |
| `scale` | How many times the frames were enlarged with sharp pixels (1 = as painted). Pixel art is usually exported at 1 for a game and at 8 for people to look at. |
| `pivot` | `[x, y]` in frame pixels (after scaling): the character's anchor point. Draw the frame so this point lands where the character stands. Default: the bottom centre of the frame, so a character stands on its feet and switching animations never moves it. The script sets it with a top-level `"pivot": [x, y]`. |
| `animations` | By name, in the order the script wrote them. `row` is its row; `frames` are its frame numbers in the grid, in play order; `fps` how fast; `loop` whether the last frame leads back to the first. |

Playing an animation: for each number in `frames`, show cell number `n` for `1 / fps` seconds;
at the end, start again if `loop`, otherwise stay on the last frame. In Duck Forge, that's
`Animation::new(frames, fps)` with the frame list.

## Other exports from the same script

- `--frames`: every frame as its own PNG, `duck-walk-003.png` (animation name, then the frame
  number with three digits).
- `--gif`: `duck-walk.gif`, looping if the animation loops, at its fps.
- `--mp4`: `duck-walk.mp4` through `ffmpeg`, on the `--bg` colour (a video has no see-through
  background; the paper colour if none is given). Without `ffmpeg`, the frames are written and
  the exact command to run is printed.
- `--scale 8` enlarges all of them with sharp pixels; `--animation walk` exports one animation.
