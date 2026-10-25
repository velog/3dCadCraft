# User interface and migration from Inventor

> **The full reference** (every mouse action, key, menu, field and setting, as they are in the code) is
> **[REFERENCE.md](REFERENCE.md)**. This file describes the idea, the layout and the migration from
> Inventor; where they differ, REFERENCE.md wins (it follows `src/gui/*.rs`). Checked against the code on
> 2026-10-19. ⬜ marks what is planned but not done yet.

Goal: a designer who works with Autodesk Inventor sits down in front of 3DCadCraft and **knows where everything is**. They find the same screen layout, the same concepts, the same mouse and keys, and the same order of work (sketch → constraints → feature).

What is copied is the **organisation and the workflow**, not the graphics: the icons, the name and the look are 3DCadCraft's own. The arrangement "ribbon with tabs + model browser + properties panel + ViewCube" is common practice in modern CAD systems.

## 1. Screen layout

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ [File] Open Save │ Undo Redo Rebuild │ Select ▾ │ Material ▾ Appearance ▾ │ Part1 * │ ← Quick Access
├──────────────────────────────────────────────────────────────────────────────┤
│  3D Model │ Sketch │ Inspect │ Tools │ Manage │ View                           │ ← ribbon tabs
│ ┌────────┐┌──────────────────────────┐┌─────────────────┐┌──────────┐        │
│ │ Start  ││ Extrude Revolve Hole ... ││ Fillet Chamfer  ││ Plane    │        │ ← command panels
│ │ Sketch ││         Create           ││     Modify      ││  Work    │        │
│ └────────┘└──────────────────────────┘└─────────────────┘└──────────┘        │
├──────────────┬──────────────────────────────────────────┬────────────────────┤
│ Model        │                                    ┌───┐ │ Properties         │
│ ▾ Bracket    │                                    │TOP│ │ ─────────────      │
│   ▾ Plate    │            3D view                 └───┘ │ Sketch1            │
│     ▸ Origin │                                 ViewCube │ DOF: 0             │
│     ▾ Sketch1│                                          │ Distance 30 mm     │
│       H, V … │                                    X/Y/Z │ …                  │
│     Body1    │                                   triad  │                    │
├──────────────┴──────────────────────────────────────────┴────────────────────┤
│ Sketch1: Fully constrained · prompt │ message │ mm ISO │ ms │ DOF Slice Align Relax │ БГ │ Mouse │
└──────────────────────────────────────────────────────────────────────────────┘
```

| Area | In Inventor | In 3DCadCraft | Status |
|---|---|---|---|
| File button + Quick Access | File menu, Quick Access Toolbar | **File** menu (New, Open, Open Recent, Save, Save As, Save as Folder, Export); buttons **Open**, **Save**, **Undo**, **Redo**, **Rebuild** (Rebuild All); **View** (User Interface: check boxes for the panels, Clean Screen, Full Screen, side of the browser and Properties, Reset UI Layout — [spec/0.13-ui-layout.md](spec/0.13-ui-layout.md)); **Select: …**; **Material** and **Appearance** lists (with `*` when faces/features/bodies have their own look); the document name (`*` = unsaved changes). There is no New button — New is in File and Ctrl+N | 🟡 |
| Ribbon with tabs | Ribbon (3D Model, Sketch, Assemble, Annotate, Inspect, Tools, Manage, View, Environments) | always the same 6 tabs: **3D Model, Sketch, Inspect, Tools, Manage, View**; Sketch is visible outside a sketch too (its commands are grey). Collapses to the tabs (`^`/`v` button or double click on the active tab; a click on a tab shows the panels temporarily). Tabs per document type (assembly, drawing) ⬜ | 🟡 shell; unfinished commands are grey with the tooltip "Planned — docs/FEATURES.md, section X" |
| Model browser | Model Browser | **Model**: project → documents → Origin, sketches, constraints, bodies; the width can be dragged and is remembered, on the left or on the right (View ▸) | 🟡 |
| Properties panel | Property Panel (replaces modal dialogs in newer versions) | **Properties**: the parameters of the selected item, editable in place; the command dialogs; the width can be dragged and is remembered, on the right or on the left (View ▸) | 🟡 |
| ViewCube | ViewCube | ViewCube in the top right corner: click on a face/edge/corner = view (smooth transition), dragging the cube = Orbit, compass below it (drag = rotation about Z, click on N/E/S/W = view from that side), house = Home, right click = menu; semi-transparent when the mouse is far away ([spec/0.3-view.md](spec/0.3-view.md)) | 🟡 |
| 3D indicator | axis triad (bottom left in Inventor) | X/Y/Z triad **bottom right** in the view | 🟡 |
| Status bar | Status Bar | sketch state + prompt for the step; message about the last operation; `mm`, `ISO`; frame time; in a sketch: drop-down for the dimension labels (Value / Name / Expression / Tolerance), buttons **DOF**, **Slice**, **Align**, **Relax**; **БГ** (prompts in Bulgarian); `Mouse: …` (the profile) | ✅ |
| Navigation Bar | Navigation Bar | Pan / Zoom / Orbit / Look At buttons | ⬜ |
| Marking menu | radial right-click menu | ordinary context menu. In the view: **Repeat <command>**, **Appearance ▸** (with objects selected), **Projection ▸**, **New Sketch**; in a sketch also **Done**, **Cancel (Esc)**, **Finish 2D Sketch**, menus for curves and dimensions. In the browser: Edit Feature, Edit Sketch / Finish 2D Sketch, Share Sketch / Unshare, Redefine, Look At, Hide/Show, Material ▸ / Appearance ▸, New Sketch, Delete (see [REFERENCE.md §2](REFERENCE.md#2-context-menus-right-click)). Radial menu ⬜ | 🟡 |
| Mini-toolbar | floating toolbar during a feature | **suggestion bar** next to a selected face/edge (New Sketch, Extrude/Press-Pull, Edit Feature, Edit Sketch, Look At; Fillet / Chamfer). Values in place in the view (e.g. the distance in Extrude) ⬜ — for now there are manipulators and fields in Properties | 🟡 |

**The ribbon (the same for every document today):**
- **3D Model, Inspect, Tools, Manage, View** and **Sketch**. Tools has a **Material and Appearance** panel (Material, Appearance, Adjust, Clear); Manage — **Styles and Standards** (Styles Editor, Update, Purge); Inspect → **Physical Properties**.
- **Sketch tab:** panels Create (with **Project Geometry**: points and curves from earlier sketches, model edges; Shift = construction; Esc / right click → Done stops it), Modify, Pattern, Constrain, Format, Demo, Exit (there is no Insert panel; Image etc. ⬜).
- ⬜ **Assembly:** Assemble, Design (calculators), Inspect, Tools, View.
- ⬜ **Drawing:** Place Views, Annotate, Sketch, Tools, View.

As in Inventor, entering a sketch opens the Sketch tab, and "Finish Sketch" returns to 3D Model and updates the model (the features that use the sketch follow it; while the sketch is open, the body stays the old one).

## 2. Mouse

There are selectable **mouse profiles** (**Tools → Application Options → Navigation**), so the hand does not have to relearn:

| Action | **Inventor** profile (default) | **Classic (left = orbit)** profile |
|---|---|---|
| Rotate (orbit) | **Shift + middle button**, or held **F4** + left | left drag anywhere, unless it starts on a sketch point, a manipulator or a pattern grip |
| Move (pan) | **middle button**, or held **F2** + left | right or middle button |
| Zoom | wheel, or held **F3** + left (up/down) | wheel |
| Select / drag a point | left button | left button (dragging only from a point) |
| Dragging a curve, a dimension's text, selection rectangle | left button | **do not work** (left drag rotates) |
| Fit (fits everything without changing the direction) | double click with the middle button | double click (any button) |
| Home view + Fit | **F6** or the ViewCube's house | the same |

- The wheel zooms about the centre of rotation, not about the cursor; the direction is reversed with *Reverse zoom direction* (also a setting in Inventor).
- F2/F3/F4 work only in the Inventor profile.
- The **Home** key (Zoom All in Inventor) is not bound ⬜.
- The profile and the wheel direction are **not remembered** between launches ⬜.

**Orbit over the pole:** vertical dragging does not stop at the top/bottom — the model rolls over and the view stays upside down; horizontal dragging then reverses its direction to follow the mouse. A click on the ViewCube (or F6) brings back an upright view. A drag started on the ViewCube or the compass rotates the view and selects nothing in the model; a drag or the wheel stops the running smooth transition.

**Snapping in a sketch:** the cursor snaps to a point within 12 px and to a curve within 8 px. The point has priority. A new line started or ended next to an existing point shares that point. A dragged point dropped onto another point gets **Coincident**. Dropped onto a curve, it gets **Coincident (on line/circle)**. The relation can be deleted from the Constraints list.

⬜ **SolidWorks** and **Fusion** profiles are planned too: they differ mainly in which button rotates.

## 3. Keyboard shortcuts

By default they are **as in Inventor**. Today they are **hard-coded**; remapping and a key file (`keymap.toml`) ⬜. The full table (with the contexts) is in [REFERENCE.md §5](REFERENCE.md#5-keyboard-shortcuts). Each key has its status in 3DCadCraft:

| Key | Action | Status |
|---|---|---|
| **F1** | the specification of the current command (`docs/spec/`) | ✅ |
| **F2** (held) + left | Pan (Inventor profile) | ✅ |
| **F2** (mouse over the browser) | Rename the selected row (feature or sketch); a slow second click does it too | 🟡 |
| dragging in the browser | feature → reorder; *End of Part* → roll back (Esc aborts) | 🟡 |
| **F3** (held) + left | Zoom (Inventor profile) | ✅ |
| **F4** (held) + left | Orbit (Inventor profile) | ✅ |
| **F5** | previous view | ⬜ |
| **F6** | Home view + Fit | ✅ |
| **F7** | Slice graphics (cuts the view through the sketch plane; in a sketch) | ✅ |
| **F8 / F9** | show / hide all constraint glyphs | ✅ |
| **F11** | Relax Mode (in a sketch) | ✅ |
| **F11** | Full Screen (outside a sketch) | 🟡 |
| **Ctrl+0** | Clean Screen: only the view and Quick Access; a second press brings everything back | 🟡 |
| **Esc** over a floating window | closes it (Parameters, Options, Physical Properties, Styles Editor, the browsers…), the command is not aborted | 🟡 |
| **Home** | Zoom All | ⬜ |
| **Align** button (status bar, in a sketch) | turns alignment to points (dashed lines) on/off | ✅ |
| **Enter** / **Space** (with no command open and no curves selected) | repeats the last command; right click → Repeat (hidden while a command is open) | 🟡 |
| **Enter** / **Ctrl+Enter** in a feature panel | Enter = OK when OK is enabled (Revolve without an axis: Continue to the axis); Ctrl+Enter = OK, also while typing in a field | 🟡 |
| letters (S, E, R, H, L, C, D, X, O) | only alone or with Shift; with Ctrl or Alt they start nothing (Alt+S is the command search) | 🟡 |
| **Ctrl+Tab / Ctrl+Shift+Tab** | next / previous document tab | 🟡 |
| **Alt+S** | the "Search commands" field in the Quick Access (↑/↓/Tab, Enter starts, Esc clears / leaves) | 🟡 |
| letters with *Show command alias input dialog* on | open the field next to the cursor; Enter / Space starts the highlighted command, Esc closes | 🟡 |
| **S** (outside a sketch) | new 2D sketch (Start 2D Sketch) | ✅ |
| **E** (everywhere; in a sketch it first leaves the sketch) | Extrude | ✅ |
| **R** | outside a sketch: Revolve; in a sketch: Two Point Rectangle | ✅ |
| **H** (outside a sketch) | Hole | ✅ |
| **L / C** (in a sketch) | Line / Center Point Circle | ✅ |
| **D** (in a sketch) | dimension (General Dimension) | ✅ |
| **X / O** (in a sketch) | Trim / Offset | ✅ |
| **Shift** (during Trim/Extend) | toggles Trim ↔ Extend | ✅ |
| **Enter** (in a sketch) | ends the chain / spline; Geometry → next step in transformations; pattern: next selector / OK; applies Edit Dimension | ✅ |
| **Delete** | deletes the selected curves and constraints — **only in a sketch**; in the 3D view it deletes nothing (features are deleted from the browser or Properties). Delete in 3D ⬜ | 🟡 |
| **Ctrl+A** | idle view: everything of the selection mode; in a sketch: all curves | ✅ |
| **double click** on a dimension | Edit Dimension | ✅ |
| **double click** on a curve (sketch) | selects the connected chain | ✅ |
| **double click** on a work feature (browser) | Redefine: the Plane/Axis/Point/UCS command opens with the current values | 🟡 |
| **E** | Extrude | ✅ |
| **R / H** | Revolve / Hole (in 3D Model) | 🟡 R ✅; H waiting for a visual check |
| **Ctrl+Enter / +** | in the Hole / Thread panel: OK / creates the feature and starts a new one with the same settings | 🟡 waiting for a visual check |
| (no key) | Coil / Rib (3D Model → Create): Coil — Profiles → Axis (automatic with a single centerline; a list for origin and work axes); Rib — Profile (the sketch curves) → Boss Centers; a click on a row in the panel makes it active, `x` clears it; Esc cancels | 🟡 waiting for a visual check |
| (no key) | Sweep / Loft (3D Model → Create): the selectors in the panel are activated by a click; Optimize for Single Selection moves on to the next one by itself; Esc cancels | 🟡 waiting for a visual check |
| Enter / Ctrl+Enter / right button | Loft: tabs Curves / Conditions / Transition; selectors Sections → Rails or Center Line → Map Point; Enter = Continue while a required input is missing, then OK; Ctrl+Enter = OK; in the menu Continue / Back / OK and Rails / Center Line / Area Loft | 🟡 waiting for a visual check |
| **P / C** | Place / Constrain (in an assembly) | ⬜ |
| **Ctrl+Z / Ctrl+Y** (and Ctrl+Shift+Z) | Undo / Redo | ✅ |
| **Ctrl+S / Ctrl+Shift+S** | Save / Save As | ✅ |
| **Ctrl+O / Ctrl+N** | Open / New (empty part; both ask about unsaved changes) | ✅ |
| **Esc** | ends the current input (command, dialog, Select Other, Edit Feature); clears the selection (→ Previous Selection); in a sketch a second Esc returns to Select | ✅ |
| **Ctrl** (held, while drawing) | no H/V/∥/⊥ inference, no tangent snap and no point alignment; snapping to points, midpoints and curves (and the constraints from it) **stays** | ✅ |
| **Ctrl** (held, while dragging a manipulator) | finer step: 0.1 mm / 1° | ✅ |

> Inventor's letter shortcuts can differ by language and version. The table will be checked against a real installation.

### Dynamic input (fields next to the cursor)

Works for Line, Center Point Circle, Two Point / Two Point Center Rectangle, Point, Move, Copy (Delta X/Y) and Rotate (Angle); details in [REFERENCE.md §4.3](REFERENCE.md#43-dynamic-input-fields-next-to-the-cursor).

| Key | Action |
|---|---|
| digits, `.`, `,` (becomes `.`), `-`, Backspace | types into the active field next to the cursor |
| Tab / Shift+Tab | next / previous field (locks what was typed) |
| Enter | places the point; the locked values become dimensions |
| Esc | clears what was typed (a second Esc ends the command) |
| ↑ | the last value typed into this field |

### Selection in 3D

| Key / action | Action |
|---|---|
| cursor over an edge/face | highlights it (green), next to the cursor: name, feature, body / length |
| click | selects (blue) and shows the suggestion bar |
| Shift+click / Ctrl+click | adds to the selection / toggles (no bar). In a sketch Shift toggles, Ctrl+click is not used |
| dragging from empty space (Inventor profile only) | rectangle: left to right — fully inside, right to left — the crossed ones too (Shift adds, Ctrl toggles) |
| double click on a face/edge | **Select Tangencies** — all tangent faces/edges |
| Ctrl+A | everything that passes the filter |
| **Select: …** (the top bar, next to Undo/Redo; also in a command) | priority: Faces and Edges / Features / Bodies / Sketch Features; **Filters ▸** (Faces, Edges, Vertices, Features, Solid/Surface Bodies, Sketches, Sketch Entities, Work Planes/Axes/Points, Origin; Select All Filters); Window/Crossing by Direction / Always Window / Always Crossing; Enable Prehighlight; Select Loop, Select Tangencies, Select Invert, Select All, Previous Selection, Clear Selection. `*` in the title = a filter is off |
| **Shift + right click** in the view | **Selection** menu next to the cursor (by context: part / command / sketch; [spec/0.9a](spec/0.9a-selection-filters.md)); does not start Pan |
| Esc | clears the selection (Previous Selection brings it back); closes Select Other |
| Extrude/Revolve: dragging from empty space | picking profiles with a rectangle |
| **Rebuild** (the top bar) / Tools → Update → Rebuild All | all features again, without the cache |
| File → **Export** → STL / STEP / GLB | file dialog with the suggested name `<name>.stl` / `.step` / `.glb` |
| Fillet/Chamfer: **Select** Edge / Loop / Feature / Solid | Loop and Feature: click on a face; Solid: All Edges / All Fillets / All Rounds |
| Fillet: Type Constant / Variable / Chord / Full Round; **Add set** | edge sets with different radii; Full Round: click on the middle face |
| Line: cursor on a circle/arc near the tangent point | tangent snap ("tangent" glyph): end on the curve + Tangent |
| resting ~0.7 s over several objects | **Select Other**: `<` `>`, ←/↑ / →/↓ or Shift+wheel cycle, a click on the name selects, Esc closes |
| Enter or Space (no active command) | repeats the last command (in Face Draft / Shell, Enter is Continue / OK) |
| БГ (bottom right) | prompts in Bulgarian |

### In an open command (Face Draft, Shell, Combine, Split)

As in Inventor's panels ([spec/0-ui.md §4.1](spec/0-ui.md)):

| Key / action | Action |
|---|---|
| a selector row in the panel | makes it active (underlined); its dot has the highlight colour in the view; `x` clears it |
| click in the view | goes to the active selector; only what it accepts is highlighted; a direction/plane moves on to the next empty selector by itself |
| click in the browser (Split) | Origin → XY / XZ / YZ, a work plane row, a sketch or Surface Bodies → surface go into the Split Tool selector; a row in Solid Bodies — into Solid |
| Split Tool active | the origin planes are shown temporarily, the one pointed at is highlighted |
| cursor in the view | label next to the mouse: what the active selector is waiting for |
| **right click** | the command's menu: **OK**, **Apply (+)**, **Continue**, **Back**, **Cancel**, **Repeat …** + the options (Draft: type, One Way / Symmetric / Asymmetric, Flip pull direction; Shell: Inside / Outside / Center, Sharp / Rounded Corners) |
| **Enter** | Continue to an empty required selector, otherwise OK (does not repeat the last command while the command is open) |
| **Ctrl+Enter** | OK |
| **Esc** | Cancel |
| Shell: the arrow in the view | dragging changes the thickness (Ctrl — finer step) |
| Shell: the name at the top (`Part > Shell1`) | a click marks it, a second click — rename |

### In an assembly (Assemble tab, [spec/3.1-assemblies.md](spec/3.1-assemblies.md))

**File → New… (Ctrl+N)** opens Create New File: tiles Part, Assembly (Sheet Metal and Drawing greyed until they come); Assembly opens a new assembly file in a tab of its own (as Inventor's `.iam`). **P** = Place:
the assembly is saved first if it is new, then a browser like Explorer lists folders and `.3dcadcraft` files
with a shaded preview of the selected part (drag it to turn it); **Place** (or a double-click) *links* the part
file — it stays its own file, saving it updates the assembly — and occurrences are named after the file
(`bracket:1`). The part is shown see-through on the cursor; click in the view for each occurrence;
right-click → OK / Place Grounded at Origin / Rotate 90° about X, Y, Z; Esc ends. Click selects a component
(Ctrl adds), a drag moves one that is not grounded, **Free Move** moves grounded ones too, **Free Rotate**:
drag inside the green circle = free turn, on its rim = turn in the screen plane. **Delete** deletes the
selected occurrences, **Esc** ends the command or clears the selection, double-click (view or browser) opens
the component's part file in its own tab. Right-click a component: Grounded,
Visibility, Open, iProperties, Copy / Paste, Delete. **V** = Free Move, **G** = Free Rotate, **J** = Joint (Type, the two origins — a face's centre, a circle's
centre, an edge's midpoint or end (white dots; **hold Shift** to keep the face under the cursor), an axis or
Origin / work geometry from the browser, shown as a red-green-blue triad — Flip, Lock
the angle / distance, Minimum / Maximum limits). **Ctrl+C / Ctrl+V**
copies components: one pasted component hangs on the cursor with the same orientation, several are pasted
beside the originals. **Assemble › Update** reads the linked part files again and solves. Every browser node
folds open and shut: *Relationships*, the assembly's *Origin*, and each component with its *Origin* (planes,
axes, centre point), its part's work features and its constraints; with Constrain open a click on such a
row is a selection (it lights up in the view under the cursor). **Use the current distance** (Constrain
panel, remembered): on, the offset is the distance the components have now; off, they snap together at 0.
The label column of Properties is wider or narrower by dragging its border. **Assemble › BOM**: the Bill of
Materials (tabs Model Data / Structured / Parts Only, Part Number Row Merge, click a header to sort, Columns for
custom iProperties, edit Part Number / Stock Number / Description / BOM Structure of the file's own components,
*Ref* per occurrence in Model Data, Export… to CSV). **C** = Constrain: the Place Constraint panel (Mate with Mate / Flush, Angle, Tangent with Inside / Outside,
Insert with Opposed / Aligned, Symmetry with a third selection — the plane), click selection 1 then 2 in the view (an edge within a few pixels wins over its face;
blue / violet / green under the cursor), Offset or Angle field, Show Preview, Predict Offset and Orientation;
OK (Enter) / Apply / Cancel (Esc), the same in the view's right-click menu. A constraint that conflicts is
refused. The browser's *Relationships* folder lists them (red = sick: its face or edge is gone); double-click
edits, right-click Suppress / Delete. Dragging a constrained component moves it only along its free degrees
of freedom (the grabbed point follows the cursor as far as the constraints allow; a component free to turn
turns). Constraints can have Minimum / Maximum limits (and Use Offset As Resting Position); right-click a
constraint or joint → **Drive...** animates its value (Start, End, Steps, Pause, Repetitions,
Start/End/Start; |< < || > >|), closing puts everything back. The ViewCube and the axis triad are in the
assembly view too. 🟡 waiting for a visual check.

### Sheet metal (Sheet Metal tab, [spec/5.6-sheet-metal.md](spec/5.6-sheet-metal.md))

File → New… → **Sheet Metal**, or 3D Model › Convert › **Convert to Sheet Metal**. **Defaults**: thickness,
K-factor, bend radius and reliefs (unticked values follow the thickness). **Face**: choose the sketch with the
closed profile, Flip. **Flange**: click sheet edges (green under the cursor, blue dots when taken; again
removes), Height, Angle, Bend Radius, Bend Position, Flip. **Flat Pattern**: the view shows the part unfolded,
Properties its extents and bends; again goes back. The other sheet metal commands are listed greyed.
🟡 waiting for a visual check.

### Help, log and recovery

The **Help** menu (on the right in Quick Access): **Help** (F1), **Save Diagnostics…** (a zip for a problem report, through the program's file dialog), **Open Logs Folder** (`%APPDATA%\3dcadcraft\logs`). The program writes a log and an action journal for every session, autosaves into `%APPDATA%\3dcadcraft\recovery` (every 5 minutes and after OK, never over the user's file) and, on a crash, `crash-….txt`. If the previous session did not close cleanly, **Restore** appears at start-up: "Restore the autosaved version of …?" — **Restore** / **Open Original** / **Discard** 🟡 ([spec/0.14-diagnostics.md](spec/0.14-diagnostics.md)).

### What is remembered between launches

`settings.json` (`%APPDATA%\3dcadcraft\settings.json` on Windows, `~/.config/3dcadcraft/settings.json` on Linux) keeps the **prompt language** (`hints_bulgarian`), the **recent files** (`recent_files`, up to 10), the **layout** (`ui`: visible panels, widths, collapsed ribbon, sides, place and size of the floating windows, size of the main window) and the **Application Options** (`options`: mouse profile, wheel direction, the sketch and inference options, the labels, the thread texture, the Home view). **Cosmetic threads** (Tapped hole, Thread) are shown as a texture of light and dark helical stripes on the face, only along the thread length; Tools → Application Options → Display → *Thread texture* turns it off (🟡 waiting for a visual check). Saving is debounced (0.5 s) and atomic; a corrupt file gives a warning and default values ([spec/0.13-ui-layout.md](spec/0.13-ui-layout.md)). Keys cannot be remapped yet ⬜ (ROADMAP U2).

## 4. Glossary for migrating users

| Inventor | 3DCadCraft | Note |
|---|---|---|
| Part (`.ipt`) | **Part** document in a project | many parts in one `.3dcadcraft` |
| Assembly (`.iam`) | **Assembly** document | components are `DocRef` by UUID, not by path |
| Drawing (`.idw`/`.dwg`) | **Drawing** document (planned) | |
| Presentation (`.ipn`) | **Presentation** (planned) | |
| Project file (`.ipj`) | the **project** itself + the linked **libraries** | no separate file with search paths |
| Pack and Go | **Pack** | embeds the libraries; the project becomes self-contained |
| Content Center | **library projects** (`*.3dcadcraft`) + Link | ordinary files, can live in git |
| Vault | git with an unpacked folder + locking and revisions (planned) | |
| iProperties | the document's **Properties** | the Physical tab = Inspect → **Physical Properties** |
| Material / Appearance Browser | **Material Browser**, **Appearance Browser** (Tools → Material and Appearance) | the libraries are CSV files; the user's is in `%APPDATA%\3dcadcraft\materials` |
| Adjust / Clear (appearance) | **Adjust**, **Clear** | precedence Face > Feature > Body > Part > Material |
| Parameters (fx) | **Parameters** | expressions with units |
| iPart / iAssembly | **configurations** (table of variants) | |
| Model States | **configurations** | shared mechanism with iPart |
| iLogic | **rules** (scripts in pure Rust, e.g. Rhai) | every action can be undone |
| Autodesk Assistant | **AI assistant through MCP** | any MCP agent can drive the program |
| Sketch constraint colors | the same meanings | green = fully constrained, blue = under-constrained |
| Derive | **Derive** / **Append** | Append = independent copy from another project |
| End of Part | **End of Part** marker | |
| Adaptivity | **adaptivity** (planned) | |

## 5. Moving existing data

1. **Models:** export **STEP AP242** from Inventor (File → Export → CAD Format) and open it in 3DCadCraft (level 7.7, planned). The geometry comes in exact but *without history*. Changes are made by direct editing (5.5) or by remodelling from the sketches.
2. **Drawings:** DXF for reference. New drawings are created associatively from the model.
3. **Standard parts:** instead of Content Center, build a company library `company-standard.3dcadcraft` (see FEATURES 6.3).
4. **Templates:** title blocks and styles are recreated in Styles & Standards (4.1).
5. **Habits:** the "Inventor" mouse profile and the keys above are on by default.

## 6. Interface language

The ribbon commands are in **English, with Inventor's names** (Extrude, Revolve, Fillet), because designers know them that way and search for them that way in the documentation.

Today **only the status-bar prompts** are translated (a built-in table in `gui/i18n.rs`): the *Hints in Bulgarian* check box in Tools → Application Options → Language or the **БГ** button in the status bar; the choice is kept in `settings.json`. Menus, dialogs and error messages stay in English. A full translation of the interface through translation files (Bulgarian and others) ⬜.

## Edit Feature

A double click on a feature in the browser (or **Edit Feature** in its context menu in the browser, in Properties, or in the **suggestion bar** after a left click on a face/edge in the view) opens its dialog with its inputs; the model rolls back to it. The right click in the view has no Edit Feature. **OK** replaces the feature in place, **Cancel** / **Esc** rolls the model forward again. The preview is computed on a background thread ("Computing the preview...").

## The Extrude and Revolve panels (after Inventor 2027)

- At the top is the **breadcrumb** `Sketch1 > Extrusion1`: a click on the sketch opens it for editing (the command waits; **Finish Sketch** brings the panel back), a click on the feature returns to it, a second click (or a double click) renames it (**Enter** accepts, **Esc** cancels).
- The **...** menu on the right is Advanced Settings: **Predict Boolean Operation** (the command itself suggests Join/Cut from the profile and the direction until you choose an operation) and **Keep sketch visible on (+)**.
- Below OK and Cancel there is **+**: it creates the feature and starts a new one (the sketch stays visible). **Ctrl+Enter** = OK.
- While the panel is open, a double click on a sketch dimension edits it in place; the preview updates.
- Right click in the view: **Continue** (Revolve: to picking the axis), **OK**, **Cancel**.
- The base feature (the first body) has no operation choice; with several bodies the preview waits for the choice of Solids or New Solid.
- **3D Model → Primitives:** Box, Cylinder, Sphere, Torus — pick a plane, then click the centre and the size (for Torus: centre, centre of the section, diameter of the section); then the Extrude or Revolve panel opens.

## Presets (ready-made sets in the panels)

Below the title of the **Extrude, Revolve, Fillet, Chamfer, Shell** and **Face Draft** panels there is a **Preset** row:

- drop-down list: **No Preset** (the default values), **Last Used** (the last OK / Apply), your own sets, after a separator — the built-in ones (read-only);
- **+** saves the current values as a new set (suggests the name `<Command> 1`; **Enter** saves, **Esc** cancels);
- **...** — Save Current, Rename Current, Delete Current, **New <Command> Default** (Last Used / Use Current Preset / No Preset), **Sort** (Recently Used / Date created / Alpha-Numeric / Last modified), Show built-in presets.

An asterisk (`*Name`) means a value was changed after the set was chosen. Sets keep only the settings, never the profiles, edges or faces. They are saved for the user (`%APPDATA%\3dcadcraft\presets\<Command>.json`, on Linux `~/.config/3dcadcraft/presets`), not in the project; the feature remembers only the set's name and Edit Feature shows it again.
