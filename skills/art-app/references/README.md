# References: pictures to learn from

Only pictures with the right to use them: public domain or CC0 (`index.json` says which, with
the source and the author; a test checks every entry). These four are photographs that ship
with the scikit-image library under those terms. Public-domain paintings from a museum or
Wikimedia Commons belong here too, but the build machine's network policy did not allow
fetching any, so none are in yet: add one by copying it here and adding an entry to the index.

For each picture: `<name>-palette.png` (its eight main colours, from `art-cli palette`),
`<name>-values.png` (in five greys, from `art-cli values`) and `<name>-breakdown.md`: the big
shapes, the values, the palette and why it works, in a few lines. `art-cli compare mine.png
<name>.png` says how close a study comes (value structure and colour, 0 to 100).

```bash
art-cli palette skills/art-app/references/coffee.png --colors 6
art-cli values skills/art-app/references/rocket.jpg
art-cli compare out/my-cup.png skills/art-app/references/coffee.png
```
