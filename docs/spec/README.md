# Feature specifications

One file per feature or mechanism. Each file describes **exactly what** the feature does in Autodesk Inventor (the functional reference, Inventor 2027) and **how far we have got**. It is used as:

- a **specification** before implementing: all inputs, settings, restrictions and edge cases;
- a **checklist**: the status of every item, ✅ done · 🟡 partial · ⬜ to do · ⛔ not applicable yet (waiting for another feature);
- a **reference** whenever it is unclear how the program should behave.

**Rules:**
- Specs are written in **English**, **in our own words**. No Autodesk text, images or file names are copied. Command and option names are kept as in Inventor's interface, for easy migration.
- Source: the Inventor Help (help.autodesk.com, `Inventor-Help`) and the API object model (`Inventor-API`), which hints at which definitions the feature has. The link is recorded at the end.
- The number in the file name points to the section in [FEATURES.md](../FEATURES.md).
- The file is updated together with the code (rule 9 in `CLAUDE.md`).

**Template:**
1. Purpose
2. Inputs (profiles, geometry, selections)
3. Settings — a table: option · Inventor behaviour · restrictions · our status
4. Behaviour in the model browser and when editing
5. Edge cases and errors
6. How we implemented it (modules, topology names)
7. Sources

## Workflow for one feature

1. **Specification:** the file here, based on the Inventor Help; every option with status ⬜.
2. **Data model** in the right layer (`kernel`, `sketch`, `part`, `document`), with tests: exact number of degrees of freedom, 0 redundant constraints, volume, Euler, orientation, as the case requires.
3. **Interface** per [0-ui.md](0-ui.md):
   - a ribbon command;
   - a prompt at every step;
   - a preview;
   - settings in Properties;
   - Undo;
   - errors in the status bar.
4. **Visual check** in the running application (the scripts from `CLAUDE.md`), with screenshots.
5. **Statuses** in the spec, in `FEATURES.md` and in `ROADMAP.md` are updated **in the same change**. Status ✅ is set only after a visual check.

## Contents

| File | Topic |
|---|---|
| [0.3-view.md](0.3-view.md) | View: Orbit over the poles, ViewCube (Home, compass, dragging, transitions), axis triad |
| [0.5-parameters.md](0.5-parameters.md) | Parameters, expressions, link to dimensions and tolerances |
| [0.6-files.md](0.6-files.md) | New, Open, Save, Save As, Export, recent files, the file dialog (also on Windows) |
| [0.6a-file-types-projects.md](0.6a-file-types-projects.md) | One extension per document kind (`.ccpart`, `.ccasm`, `.ccdrw`, `.ccpres`), design projects (`.ccproj`) |
| [0.6b-multi-document-files.md](0.6b-multi-document-files.md) | Many documents in one design file (New Part / Assembly / Drawing in this file, Documents panel) and separate files (extract, embed, pack, unpack) |
| [0.17-home.md](0.17-home.md) | Home: the start page — active project, New / Open, recent documents, learning links |
| [0.10-physical.md](0.10-physical.md) | Physical Properties: mass, volume, area, centre of gravity, inertia |
| [0.9-selection.md](0.9-selection.md) | Selection, filters, prehighlight, Select Other, context suggestions |
| [0.15-context-menus-shortcuts.md](0.15-context-menus-shortcuts.md) | Right-click by object kind and place, marking menu, keys and aliases, alias field, command search (Alt+S), mouse, accessibility; plan of the groups |
| [0.9a-selection-filters.md](0.9a-selection-filters.md) | One selection model: everything + filters, Select Priority, intersection with the command's selector, Shift + right-click menu |
| [0.14-diagnostics.md](0.14-diagnostics.md) | Diagnostics: session log, action journal and replay, autosave and Restore, crash report, Save Diagnostics |
| [0.15-context-menus-shortcuts.md](0.15-context-menus-shortcuts.md) | Right-click, keys, aliases, command search, command registry (G0) |
| [0.11-presets.md](0.11-presets.md) | Presets: ready-made sets of settings in command panels (Last Used, No Preset, built-in) |
| [4-drawings.md](4-drawings.md) | Drawings: sheets, views (HLR), annotations, tables, output — research document and stages D1–D6 |
| [4.4-parts-list.md](4.4-parts-list.md) | Assembly views in drawings, Parts List (columns from any property, overrides by part number, sort / renumber, hidden and custom rows, wrapping), Balloon, Auto Balloon |
| [4.8-sheet-metal-drawings.md](4.8-sheet-metal-drawings.md) | Sheet metal in drawings: flat pattern views (Bend Centerline / Bend Extent layers), Bend Notes; bend tables next |
| [4.9-format-text.md](4.9-format-text.md) | Format Text for notes: runs (font, size, colour, styles), justification, spacing, rotation, border / fill, symbol palette, live fields (stage D-S2) |
| [4.10-break-view.md](4.10-break-view.md) | Break view: Rectangular / Structural breaks, gap, display, symbols, true dimensions across a break (stage D-S6) |
| [4.6-drawing-styles.md](4.6-drawing-styles.md) | Drawing styles: the Style and Standard Editor of drawings (dimension, text, leader, centre mark, hatch, view annotation styles, layers, Object Defaults) |
| [4.7-drawing-commands.md](4.7-drawing-commands.md) | The whole Inventor drawing command set (Place Views, Annotate, Manage): each command's dialog first, logic later (parts list / BOM source, balloons, dimension sets, tolerances, symbols, welding, hole tables, revisions, model reference) |
| [6.1-frame-generator.md](6.1-frame-generator.md) | Frames (Frame Generator) — research document and stages F1–F4 |
| [6.3-content-center.md](6.3-content-center.md) | Library of standard components (Content Center) — research document and stages L1–L5 |
| [5.6-sheet-metal.md](5.6-sheet-metal.md) | Sheet Metal: rules, features, flat pattern, DXF — research document and stages (nothing implemented yet) |
| [3.0-assembly-commands.md](3.0-assembly-commands.md) | The whole assembly command set (Assemble, Design, Inspect, Manage, View, Environments) with every dialog, interface first |
| [3.6-bom.md](3.6-bom.md) | Bill of Materials: Model Data / Structured / Parts Only, BOM Structure, merge, Export |
| [0.16-iproperties.md](0.16-iproperties.md) | iProperties: General, Summary, Project, Status and custom properties of every document |
| [0.13-ui-layout.md](0.13-ui-layout.md) | Interface layout: panel sizes, ribbon collapsing, User Interface, Clean Screen, Full Screen, floating windows, `settings.json` |
| [0-ui.md](0-ui.md) | Interface: look, colours, glyphs, command behaviour, selection, guiding the user, performance |
| [1.1-sketch.md](1.1-sketch.md) | 2D sketch: creation, editing, coordinate system, projections, settings |
| [1.2-sketch-geometry.md](1.2-sketch-geometry.md) | Drawing tools, Fillet and Chamfer, Format |
| [1.2a-splines.md](1.2a-splines.md) | Splines: Interpolation, Control Vertex, Equation, Bridge; handles, methods, curvature |
| [1.2b-ellipse.md](1.2b-ellipse.md) | Ellipse and elliptical arcs: creation, dimensions, constraints, Trim / Offset, exact faces |
| [1.3-constraints.md](1.3-constraints.md) | Geometric constraints, degrees of freedom, colours, Relax Mode, solver |
| [1.4-dimensions.md](1.4-dimensions.md) | Dimensions: General Dimension, Edit Dimension, driven, display, tolerances |
| [1.5-inference.md](1.5-inference.md) | Snapping and automatic constraints |
| [1.6-modify.md](1.6-modify.md) | Trim, Extend, Split, Offset, Move, Copy, Rotate, Scale, Stretch |
| [1.7-patterns.md](1.7-patterns.md) | Rectangular, Circular, Mirror |
| [1.8-project-geometry.md](1.8-project-geometry.md) | Project Geometry |
| [1.9-profiles.md](1.9-profiles.md) | Profiles and regions |
| [2.0-part-commands.md](2.0-part-commands.md) | The whole Inventor part command set (3D Model, Inspect, Tools, Manage, View, Environments) with every dialog, interface first; right-click menus by object kind (rule 17) |
| [5.4-surfaces.md](5.4-surfaces.md) | Stitch, Boundary Patch, Trim, Extend, Ruled Surface, Replace Face, Sculpt as `BodyOp` variants (format revision 37) |
| [5.4a-surface-workflow.md](5.4a-surface-workflow.md) | Review of surface work: highlight, visibility (Alt+V), Trim preview, Loft / Patch between surfaces, Creo-like chains; the order of work |
| [2.14-delete-face.md](2.14-delete-face.md) | Delete Face: Individual Faces (Heal or open into a surface) or a whole Lump or Void; `BodyOp::DeleteFace` |
| [2.14-thicken-offset.md](2.14-thicken-offset.md) | Thicken/Offset: faces or quilts thickened (Join, Cut, Intersect, New Solid) or offset into a surface; `Feature::BodyOp` |
| [2.1-extrude.md](2.1-extrude.md) | Extrude, including consumed and shared sketches |
| [2.2-revolve.md](2.2-revolve.md) | Revolve, axis and centreline, diametric dimensions |
| [2.3-hole.md](2.3-hole.md) | Hole: Simple / Clearance / Tapped, seats, terminations, standards in CSV, drawing note |
| [2.4-fillet.md](2.4-fillet.md) | Fillet (rounding of edges) |
| [2.5-chamfer.md](2.5-chamfer.md) | Chamfer |
| [2.8-draft.md](2.8-draft.md) | Face Draft: Fixed Edge, Fixed Plane, pull direction |
| [2.6-shell.md](2.6-shell.md) | Shell: Remove Faces, Inside / Outside / Both, closed cavity |
| [2.7-rib.md](2.7-rib.md) | Rib / Web: ribs and webs from open profiles, To Next / Finite, Draft, bosses, networks |
| [5.3-coil.md](5.3-coil.md) | Coil: springs and helical grooves, three methods, Taper, closed ends, thread with Cut |
| [3.1-assemblies.md](3.1-assemblies.md) | Assemblies: placing components, constraints (Mate, Flush, Angle, Tangent, Insert, Symmetry), joints; one solver for both; what is taken from SolidCraft; plan and cost |
| [3.10-top-down.md](3.10-top-down.md) | Top-down design: Create in place, Edit in place, cross-part references and Update, adaptivity, Derive, Make Part / Make Components, layouts and skeletons, assembly features, linked parameters; order of work T0–T10 |
| [3.1a-subassemblies.md](3.1a-subassemblies.md) | Subassemblies: an assembly placed in an assembly (Rigid / Flexible), nested display and picking, relationships into nested parts, BOM levels, Interference, Demote / Promote |
| [5.1-sweep.md](5.1-sweep.md) | Sweep: profile along a path (sketch or edges), Guide Rail, Parallel, Taper, Twist |
| [5.2-loft.md](5.2-loft.md) | Loft: sections (sketches, faces, point), Closed Loop, conditions, transition |
| [2.11-patterns.md](2.11-patterns.md) | Rectangular Pattern and Circular Pattern of features and bodies |
| [2.12-mirror.md](2.12-mirror.md) | Mirror of features and bodies |
| [2.9-thread.md](2.9-thread.md) | Thread: cosmetic or modeled thread, clearance for 3D printing |
| [2.10-work-features.md](2.10-work-features.md) | Work planes, axes, points |
| [2.13-booleans.md](2.13-booleans.md) | Join / Cut / Intersect, Surface, representation of the result |
| [2.13-move-bodies.md](2.13-move-bodies.md) | Move Bodies: Free Drag, Move Along Ray, Rotate About Line, several moves; `BodyOp::MoveBodies` |
| [2.13-copy-object.md](2.13-copy-object.md) | Copy Object: faces or bodies into surfaces, a composite, a solid or solids; Delete Original; `BodyOp::CopyObject` |
| [2.13-combine-split.md](2.13-combine-split.md) | Combine (Base, Toolbodies, Keep Toolbody) and Split (Split Face, Trim Solid, Split Solid) |
| [2.15-materials.md](2.15-materials.md) | Materials, appearances, overrides, libraries, Styles Editor |
| [2.16-browser.md](2.16-browser.md) | Model browser: order, nested sketches, bodies, End of Part, reordering, Suppress, Delete, Rename |
| [7.7-export.md](7.7-export.md) | Export: STL, STEP, GLB |

Test matrix of the main features: [TESTING.md](TESTING.md).
