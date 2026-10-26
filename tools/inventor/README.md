# Inventor reference scripts

Autodesk Inventor 2027 is installed on the owner's Windows machine; these PowerShell scripts drive it
through its COM API (hidden, `SilentOperation`) to read reference values for 3DCadCraft. They never
change Inventor's templates or settings: documents are created from the templates and closed without
saving (the reference drawing is saved next to the output). A hidden Inventor instance (one a script
left behind) is closed with `Quit` at the end.

| Script | Output |
|---|---|
| `styles.ps1 OUT.json TEMPLATE.idw...` | every dimension / text / leader / centre mark / hatch style and layer of the templates (some property names are guesses; `members.ps1` lists the real ones) |
| `members.ps1 OUT.txt TEMPLATE.idw` | every property of the standard, the default dimension, text, leader, centre mark, hatch styles and Object Defaults |
| `border.ps1 OUT.txt` | the ISO template's border (margins, zones, labelling) and title block per sheet size |
| `refdrawing.ps1 DIR` | the 80 × 50 × 12 plate with holes (as `examples/drawing_demo.rs`), an ISO drawing with a front view 1.5 : 1, the projected view, a diameter dimension and a centre mark; dimension texts in `refdrawing.txt`, `plate_ref.pdf` |

Values are in Inventor's internal units (cm); the curated values in mm live in
`tests/data/inventor/styles_2027.json` and `drawing::tests::defaults_match_inventor_2027` checks our
defaults against them. Run (PowerShell):

```powershell
$t = "C:\Users\Public\Documents\Autodesk\Inventor 2027\Templates\en-US\Metric"
powershell -NoProfile -ExecutionPolicy Bypass -File tools\inventor\members.ps1 out.txt "$t\ISO.idw"
```

Findings (2026-10-26): ISO dimensions write `Ø11,00` (decimal comma, 2 decimals, trailing zeros);
arrows 2.5 × 1 mm; extension 3.175, origin offset 0, gap 0.762; Note Text Tahoma 3.5 mm; centre mark
5 / 1.5 / 2.5 on a *continuous* layer; layers visible 0.5, hidden 0.35, hatch 0.18, section line 0.7
chain, border 0.7, title 0.5; border 10 mm (20 mm on A0 / A1), 6 × 4 zones labelled from the bottom
right, centring marks; title block 170 × 30 mm. Inventor's Front view looks at the XY plane (Y up,
`FrontViewPlane` of the standard); ours looks at XZ (Z up).
