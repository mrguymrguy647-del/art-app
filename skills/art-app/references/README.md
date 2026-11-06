# References: pictures to learn from

Only pictures with the right to use them: public domain or CC0 (`index.json` says which, with
the source and the author; a test checks every entry). Five are public-domain paintings from
Wikimedia Commons (photographs of two-dimensional works whose artists died more than a hundred
years ago), four are public-domain and CC0 photographs that ship with the scikit-image library.
Add one by copying it here, shrinking it to about 1024 px wide, and adding an entry to the
index with its source and licence.

For each picture: `<name>-palette.png` (its eight main colours, from `art-cli palette`),
`<name>-values.png` (in five greys, from `art-cli values`) and `<name>-breakdown.md`: the big
shapes, the values, the palette and why it works, in a few lines. `art-cli compare mine.png
<name>.jpg` says how close a study comes (value structure and colour, 0 to 100).

```bash
art-cli palette skills/art-app/references/vermeer-girl-with-a-pearl-earring.jpg --colors 6
art-cli values skills/art-app/references/friedrich-wanderer.jpg
art-cli compare out/my-wave.png skills/art-app/references/hokusai-great-wave.jpg
```
