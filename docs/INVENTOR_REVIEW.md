# Review against Inventor 2027 — workflow, interface, gaps (2026-10-29)

An analysis for planning the next work; nothing here is implemented by this document. Sources: the
statuses in `docs/spec/*.md` and `docs/FEATURES.md`, the command registry (`gui/commands.rs`), the code,
the owner's session logs (2026-10-19 … 10-27) and a scripted run of the app on 2026-10-29 (part, sketch,
assembly, drawing, Home, the owner's `Кутия` + `Assembly1`). Inventor 2027 is the functional reference
(how a user works), never a source of text or data.

## 1. The overall picture

- **Breadth is far ahead of acceptance.** Almost every environment has its commands, panels and much of
  the logic, but most spec rows are 🟡 "done, waiting for a visual check". Specs with no ✅ row at all
  although their logic is in: Hole (54 🟡), Thread, Shell, Rib, Draft, Mirror, Feature Patterns, Delete
  Face, Move Bodies, Coil, Sweep, Materials, Physical Properties, Presets, Files, Multi-document files,
  Home, Diagnostics. Sheet metal: 4 ✅ / 50 🟡 / 81 ⬜. A user meets these as "probably works".
- **`docs/FEATURES.md` headings are stale** in places: 0.5 Parameters says ⬜ (the spec is 44 % ✅),
  3.8 Replace / Copy says ⬜ (Replace, Copy, Mirror exist: `assembly::replace`, `gui/asm_replace.rs`),
  2.1 Extrude says "all settings except the Boolean operations" (they exist). The roadmap reads from it.
- Strongest areas (share of ✅ rows): sketch patterns 80 %, constraints 75 %, dimensions 75 %, Modify
  70 %, sketch geometry 68 %, UI rules 65 %. Weakest among the implemented ones: drawing styles 1 %,
  sheet metal 2 %, loft 2 %, assemblies 3.1 1 %, part planned commands 2.0 3 % (63 ⬜).

## 2. Workflow compared with Inventor

| Area | How an Inventor user works | Here now | Gap |
|---|---|---|---|
| New file | New → template per kind and standard (Standard.ipt, ISO / ANSI / DIN …; units, styles, title block come from it); project file `.ipj` with search paths | Create New File: Part / Assembly / Drawing, no template or standard choice; design projects (spec 0.6a: 4 🟡, 24 ⬜) | templates per standard; project search paths; Pack and Go |
| Bringing old work | open / import STEP, IGES, SAT, DWG, Inventor files of other versions; AnyCAD references | export STL / STEP AP242 / GLB / flat DXF; **no import of any format** | **STEP import is the migration blocker** (an Inventor user cannot bring existing parts) |
| Sketch | 2D + 3D sketch, Ellipse, Text, Insert Image / Points (Excel), sketch blocks | 2D complete and the most mature area; Ellipse and Text are disabled in the ribbon; 3D sketch ⬜ (1.10); blocks / images ⬜ (1.11) | 3D sketch (also needed by sweeps along 3D paths, Frame Generator, Tube & Pipe); Ellipse; Text (Emboss depends on it) |
| Feature commands in the view | mini-toolbar next to the cursor (OK / Apply / Cancel, the main option, the value box); radial marking menu on right click; double-click a dimension in the view | value boxes and manipulators in the view, Show Dimensions (`gui/show_dims.rs`), list context menus (rule 17), Repeat, a small toolbar next to a selected face / edge (New Sketch, Fillet, Chamfer, Edit Feature, Look At) | **no mini-toolbar inside a running command, no marking menu**: OK / Apply / Cancel and the options are only in Properties; an Inventor user expects to stay in the view |
| Part modelling | Extrude … Thread, Emboss / Decal, Derive, iFeatures, iParts / iAssemblies, model states, Freeform, Direct Edit, plastic part | the core features and surfaces exist (🟡); Emboss / Decal ⬜ (2.17), Derive / iParts / iFeatures ⬜ (5.7), Freeform / Direct ⬜ (5.5), Plastic ⬜ (5.8) | Emboss (needs sketch Text), Derive (part into part — also the base of top-down), model states |
| Assemblies | Place / Constrain / Joint, drag with Contact Solver, Drive, Content Center fasteners, Bolted Connection, representations (view / positional / LOD), model states, top-down (in-place, adaptive), Frame Generator | Place, Constrain, Joints, Drive (`gui/drive.rs`), Replace / Copy / Mirror, patterns, BOM, interference, subassemblies; Content Center 6 %, Design tab 24 commands panel-only, top-down 8 %, representations ⬜ (3.9), contact ⬜ (3.4) | **standard parts (screws, nuts, washers) are daily use** — Content Center + Bolted Connection; representations; contact dragging |
| Drawings | template with styles / title block; Base / Projected / Section / Detail; Retrieve Dimensions; hole / revision / parts tables; Update; DWG/PDF | views, sections, details, dimensions, tables, balloons, PDF; styles 1 % ✅, `4-drawings` 42 ⬜; Define Border / Title Block / Symbol, Update All, Replace Model are panels only | the drawing **template** (own title block, styles saved as a standard) and DWG / DXF of a sheet |
| Automation | iLogic rules, forms, the API, Vault | Parameters with units, Suppress if, equations; iLogic 7.5 ⬜; **API / MCP 0.7 ⬜ — `src/api` is a placeholder** | the agent API is this project's own reason to exist and has not started |

## 3. Interface findings from the run (2026-10-29)

Fixed the same day (2175102, ef21098): the ribbon folded panels while ~200 px stayed empty; the Quick
Access Toolbar showed Select / Material / Appearance in a drawing; Edit Feature marked the file changed;
Home's empty list and Create New File were misaligned; idle Properties said "Box"; switching to an
assembly tab froze the app ~0.8 s each time (every linked part was rebuilt).

Still open:

1. ✅ (later the same day) Hover text and Select Other show *kind · feature · body*; the persistent names
   (`Edge E[Extrusion1:End|…]`) only with Application Options → Display → *Internal names in hover text*.
2. ✅ Select Other closes when a window opens over the view (it stayed on top of the Open dialog).
3. **Constrain is folded at 1400 px in the sketch**: open it is ~250 px and does not fit even with every
   other panel folded (panels now open by importance, so it is the first to open on a wider window).
   Narrower: Dimension as a small button, or the constraint glyphs in two rows instead of three.
4. ✅ `Axis edge` in the sketch's Coordinate System (the grid uses the shared label column). Still cut:
   the status-bar navigation hint (`Shift+Middle / F4: or…`).
5. Saving while Edit Feature is open writes the rolled-back body into the file (it is rebuilt on open,
   so nothing is lost, but the file is not what the user sees after OK).
6. In an assembly the frame cost shown right after opening was 55 ms; not reproduced as slow frames in
   the log afterwards — recheck on a larger assembly.

## 4. Stability

- One crash since 10-26 (10-27, GPU validation error then egui-wgpu panic). The renderer now keeps
  buffers and textures within the device limits and logs GPU errors; 2 sessions ended without a clean
  exit (10-26 08:36, 10-27 11:30) with no crash report — likely killed by the user or an abort.
- Frames over 250 ms are now logged as `slow frame` (ef21098): ask the owner for logs after real work.
- Open from `ROADMAP.md` "Critical fixes": K1–K3 (slow frames over a modeled thread, Slice through it,
  every facet edge drawn in the thread zone), K4 (countersinks off a square face), K5 (timing of
  `hollow_box` under parallel tests).
- `cargo xtask quick` and `ci` green on 2026-10-29; 9 ignored tests (6 "large, release only", 1 kernel
  pin, 1 Stitch dome), 1 oracle known failure.

## 5. Suggested order for the next work

1. **Acceptance sweep** (no new features): go environment by environment through the 🟡 rows with the
   owner, turn them ✅ or record the defect; correct the FEATURES headings. This is what the owner sees.
2. **Migration blockers**: STEP import (exact faces where the kernel can, named by our persistent-name
   rules); New from a template with a standard (ISO first); the drawing template (title block, styles).
3. **Daily-work speed in the view**: a mini-toolbar for feature commands (OK / Apply / Cancel + the
   main value), optionally a marking menu; internal names off the hover text; Constrain unfolded at
   1400 px; sketch Ellipse and Text, then Emboss.
4. **Standard parts**: a minimal Content Center (ISO screws, nuts, washers from our own tables, like the
   hole standards) and Bolted Connection on top of Hole; then representations and contact dragging.
5. **3D sketch**, then Derive (part into part), which opens top-down work and Frame Generator.
6. **API / MCP** (0.7): the command layer (`Command` enum, ROADMAP K8) the GUI and agents share.
