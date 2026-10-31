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
- **3D Model, Inspect, Tools, Manage, View** and **Sketch**. Tools has a **Material and Appearance** panel (Material, Appearance, Adjust, Clear); Manage — **Documents** (the design file's documents: New Part / Assembly / Drawing in this file, Duplicate, Rename, Delete, the Documents window), **Storage** (Save to Separate File, Embed, Embed All, Separate All) and **Styles and Standards** (Styles Editor, Update, Purge); Inspect → **Physical Properties**.
- **Sketch tab:** panels Create (with the **Project Geometry** split button: Project Geometry — points and curves from earlier sketches, model vertices, edges and faces, work features, and while a part is edited in place the edges and faces of the other components; Shift = construction; Ctrl = the whole loop of the edge under the cursor (on the face parallel to the sketch); Esc / right click → Done stops it —, **Project Cut Edges** (PCE) and **Project Flat Pattern** (PFP)), Modify, Pattern, Constrain, Format, Demo, Exit (there is no Insert panel; Image etc. ⬜).
- 🟡 **Assembly:** Assemble, Design (calculators), Inspect, Tools, Manage, View, Environments (2026-10-27: every command on the ribbon, spec 3.0).
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

- The wheel zooms **towards the cursor** (*Zoom to cursor*, on by default; off: about the centre of the view); the direction is reversed with *Reverse zoom direction* (also settings in Inventor).
- **Orbit pivot:** an orbit that starts over a face turns about that point (a small centre mark shows it, the point stays under the cursor); over empty space it turns about the centre of the view (*Orbit about the model point under the cursor*).
- **Default Orbit Type:** *Free* (default, Inventor's): relative to the screen — from a top view a horizontal drag tilts the model, it does not spin it about Z; *Constrained*: a horizontal drag turns about the model's Z.
- **Zoom out to new geometry:** when the edited sketch grows out of the view (typed sizes, dimensions) or a placed component does not fit, the view zooms out to everything by itself (*Zoom out when new geometry leaves the view*).
- F2/F3/F4 work only in the Inventor profile.
- The **Home** key (Zoom All in Inventor) is not bound ⬜.
- The profile, the wheel direction and the navigation options above are remembered in `settings.json`.

**Orbit over the pole:** vertical dragging does not stop at the top/bottom — the model rolls over and the view stays upside down; horizontal dragging then reverses its direction to follow the mouse. A click on the ViewCube (or F6) brings back an upright view. A drag started on the ViewCube or the compass rotates the view and selects nothing in the model; a drag or the wheel stops the running smooth transition.

**Snapping in a sketch:** the cursor snaps to a point within 12 px and to a curve within 8 px. The point has priority. A new line started or ended next to an existing point shares that point. A dragged point dropped onto another point gets **Coincident**. Dropped onto a curve, it gets **Coincident (on line/circle)**. The relation can be deleted from the Constraints list.

⬜ **SolidWorks** and **Fusion** profiles are planned too: they differ mainly in which button rotates.

## 3. Keyboard shortcuts

By default they are **as in Inventor**. Your own keys: **Tools → Customize → Keyboard** (every command with its aliases and shortcut; click a shortcut and press the keys, type aliases separated by commas; Reset / Reset All; Copy to Clipboard / Import as JSON), or **right-click any ribbon button** → *Assign Shortcut…*, *Change Alias…*, *Remove Shortcut*, *Remove Alias*, *Reset to Default* (as in Blender). They are kept in `settings.json` → `options.keymap` (only what you changed) and win over the built-in keys. The full table (with the contexts) is in [REFERENCE.md §5](REFERENCE.md#5-keyboard-shortcuts). Each key has its status in 3DCadCraft:

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
| **F11** | Full Screen (outside a sketch; kept for the next start) | 🟡 |
| **Ctrl+0** | Clean Screen: only the view and Quick Access; a second press brings everything back | 🟡 |
| **Esc** over a floating window | closes it (Parameters, Options, Physical Properties, Styles Editor, the browsers…), the command is not aborted | 🟡 |
| **Home** | Zoom All | ⬜ |
| **Align** button (status bar, in a sketch) | turns alignment to points (dashed lines) on/off | ✅ |
| **Enter** / **Space** (with no command open and no curves selected) | repeats the last command; right click → Repeat (hidden while a command is open) | 🟡 |
| **Enter** / **Ctrl+Enter** in a feature panel | Enter = OK when OK is enabled (Revolve without an axis: Continue to the axis); Ctrl+Enter = OK, also while typing in a field | 🟡 |
| letters (S, E, R, H, L, C, D, X, O) | only alone or with Shift; with Ctrl or Alt they start nothing (Alt+S is the command search) | 🟡 |
| **Ctrl+Tab / Ctrl+Shift+Tab** | next / previous document tab | 🟡 |
| **Alt+S** | the "Search commands" field in the Quick Access (↑/↓/Tab, Enter starts, Esc clears / leaves) | 🟡 |
| *Select Loop / Select Tangent Chain / Select Face Boundary / Select From-To* | Right-click menu of a surface command whose selector takes edges: the boundary loop through the edge under the cursor, the edges going on smoothly from it, the outer loop of the face there, or the edges along the boundary from the last picked edge to the one under the cursor (the shorter way); a box from empty space adds what the selector takes | 🟡 |
| **Alt+V** | Visibility of the selected bodies / surfaces (of the bodies of selected faces): hides them, or shows them when hidden (a hidden body picked in the browser comes back); also *Visibility (Alt+V)* in their right-click menu, and *Show All Bodies* in the view's menu while some are hidden | 🟡 |
| letters with *Show command alias input dialog* off (default) | multi-letter aliases gather in the status bar (`Command: SM_  (Flange)`); an alias no longer one starts with runs at once (`SMF`, `E`), one with longer ones after 0.6 s or Enter / Space (`S`); Backspace, Esc clears; red when nothing matches | 🟡 |
| right-click a ribbon tab | **Show Tabs** (turn tabs on / off, Show All), **Minimize Ribbon** | 🟡 |
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

**Further aliases** (2026-10-26; type them over the view, the tooltips show them; the command search
finds every command by its alias too):

- 3D Model: CO Coil, RI Rib, CMB Combine, SPL Split, UCS UCS, BX Box, CY Cylinder, SPE Sphere, TOR Torus.
- Sketch: TC Tangent Circle, AC Center Point Arc, RC / RCT Two / Three Point Center Rectangle, SL / SLO / SLC
  Center to Center / Overall / Center Point Slot, SLA / SLR Three Point / Center Point Arc Slot, SPV Control
  Vertex Spline, CHA Chamfer, CO Copy, RO Rotate, SPS Split, SC Scale, STR Stretch, PTH Pattern along Path,
  PPT Pattern at Points, CN Coincident, CL Collinear, CC Concentric, GND Fix, PA Parallel, PE Perpendicular,
  TA Tangent, SY Symmetric, ECS Edit Coordinate System, CT Construction, CE Centerline, NO Normal, CPT Center
  Point, CSE Constraint Settings, ESK Finish Sketch, QDR / QDS / QDT / QDL / QDV the demos.
- Inspect / Tools / Manage: PPR Physical Properties, IVS Validate Solid, AO Application Options, TDS Document Settings, MAT Material,
  APP Appearance, ADJ Adjust, APC Clear, PRM Parameters, RBA Rebuild All, STE Styles Editor, UST Update
  Styles, PUR Purge Styles.
- Manage › Documents / Storage (spec 0.6b; part, assembly and drawing, not in a sketch): DM Documents,
  NP / NA / ND / NPN New Part / Assembly / Drawing / Presentation in This File (a part takes sheet metal
  features at any time: one part type),
  IDF Insert Document, LF Link File, DUP Duplicate, RND Rename, DDC Delete Document, XF Save to Separate
  File, MKI Embed, PK Embed All, UPK Separate All.
- View: NFR / NTP / NRT Front / Top / Right, NOR / NPE / NPO Orthographic / Perspective / Perspective with
  Ortho Faces, NVO / NVP / NVA / NVT / NVU visibility of origin planes / work planes / axes / points / UCS,
  UIB / UIP / UIR / UIS / UIV / UIT Browser / Properties / Ribbon / Status Bar / ViewCube / Axis Triad,
  UIL Reset UI Layout, UIM Minimize Ribbon.
- Assembly: FIX Grounded, UPD Update, BOM Bill of Materials.
- File / Help: NDW New Drawing, NAS New Assembly, SVF Save as Folder, STL / STP / SGL Export STL / STEP /
  GLB, SDG Save Diagnostics, SLG Open Logs Folder.

E and H (3D Model) and L, D, O, X (sketch) have no longer alias starting with them, so they start at
once (a test keeps it so).

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

**File → New… (Ctrl+N)** opens Create New File: tiles Part, Assembly, Drawing (sheet metal is made in a part); Assembly opens a new assembly file in a tab of its own (as Inventor's `.iam`). **P** = Place:
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
**The whole assembly command set** ([spec/3.0](spec/3.0-assembly-commands.md), interface only — OK disabled): tabs
Assemble (Component: Place from Content Center **PCC**, Place Imported CAD Files **PIF**, Create **CRC**, Replace
**RC** / **RA**; Relationships: Assemble **ASM**, Show **SHR**, Show Sick **SSR**, Hide All **HAR**; Pattern **PCO**,
Mirror **MIC**, Copy **CPC**; Productivity: Make Layout **MKL**, Create Derived Substitutes **CDS**; Simplify
**SIM**), **Design** (Bolted Connection **BC**, Clevis / Secure / Radial Pin **CVP SCP RDP**, Insert Frame **IFR**,
Insert End Cap **FEC**, Change **FCH**, Frame Member Info **FMN**, Miter **FMI**, Notch **FNO**, Corner Joint **FCJ**,
Trim/Extend **FTE**, Lengthen/Shorten **FLS**, Reuse / Change Reuse **FRU FCR**, Remove End Treatments **FRE**, Refresh
**FRF**, Edit Frame **FED**, Beam and Column Calculator **BCC**,
Shaft **SHF**, Spur / Worm / Bevel Gear **SPG WGR BVG**, Bearing **BRG**, Key **KEY**, V-Belts **VBT**, Synchronous
Belts **SBT**, Roller Chains **RCH**, Disc / Linear / Cylindrical Cam **DCM LCM CCM**, Involute / Parallel Splines
**ISP PSP**, O-Ring **ORG**, Compression / Extension / Torsion / Belleville Spring **CSP ESP TSP BSP**), Inspect ›
Analyze Interference **INT**, Manage › Create iAssembly **CIA**, View › Degrees of Freedom **DOF**, Environments
(Tube & Pipe **TAP**, Cable & Harness **CAH**, Dynamic Simulation **DSI**, Stress Analysis **STA**, Studio **STU**,
Presentation **PRS**). Each opens its panel in Properties: selectors take a component (click it) or a face / edge of
one; the selected components fill the first selector; Enter / right-click Continue / Esc as in the other
commands. A component's right-click menu offers Replace, Pattern, Mirror, Copy and Analyze Interference with it.

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

Every sheet metal command opens a property panel like Inventor's (2026-10-26): the name (click to rename),
**Input Geometry** selector rows (the active row takes the clicks in the view: hover lights green, chosen items
blue; a single pick moves on; `x` clears a row), the command's sections, **OK / Apply / Cancel**. The result is
shown see-through orange while values change (computed in the background). Arrows drag the Flange height and
width offsets, the Hem length and the Contour Flange distance. Right-click: OK, Apply, Continue, Back, Flip,
Cancel; Enter = Continue / OK, Esc = Cancel. Double-click a sheet metal feature in the browser to edit it in
the same panel. Edges (Flange, Hem): a second click on a chosen edge switches it to the same edge on the other
face of the sheet (the flange turns to that side), Ctrl+click takes it out. Flange's **Corner** section: corner
relief shape and size and the miter gap of this flange, and **Corner Overrides** for each corner (numbered dots in
the view). **Corner Seam** (Modify): every corner is a numbered dot — click one (or its row in the panel), choose
Symmetric Gap / Overlap / Reverse Overlap, gap, percent overlap. **Cut**: Extents Thickness / Distance (a pocket)
/ Through All, or Cut Across Bend. The ribbon stays on the tab you chose (Finish Sketch returns to the tab the
sketch was opened from).

Every part has the Sheet Metal tab (after 3D Model; no separate sheet metal part type): **Face** or 3D Model › Convert › **Convert to Sheet Metal** starts one. The tab also has Start 2D Sketch, Hole, the patterns (Rectangular, Circular, Mirror) and the work features (Plane, Axis, Point, UCS). **Defaults**: **Material** (filter) and
**Rule** from the rule table (it fills the values and, on OK, the part's material), **Edit Rules File...** (opens the
table in Excel: one rule per row; save it, then **Reload**); thickness,
K-factor, bend radius and reliefs (unticked values follow the thickness). **Face**: choose the sketch with the
closed profile, Flip. **Flange**: click sheet edges (green under the cursor, blue dots when taken; again
removes), Height, Angle, Bend Radius, Bend Position, Flip. **Width Extents** of a Flange: Edge, Width (centred or from an offset) or Offset; the orange arrows at
the flange's ends drag it narrower (it becomes Offset), the orange lines are the preview. **Convert to Sheet
Metal** (Setup, or 3D Model › Convert) on a solid: click the base face (orange dot), then the edges that
become bends (blue dots; again removes), OK. Defaults has the **Corner Relief Shape** (Round, Square, Tear,
Trim to Bend). **Hem**: click sheet edges, Length, Gap, Flip. **Fold**: choose the sketch and its line (on
a face of the sheet), Angle, Bend Radius, Fold Location, Flip Side / Flip Direction. **Contour Flange**: a
sketch with an open chain of lines, Distance (width), Flip. **Cut**: a sketch with closed profiles on a face of
the sheet or on a work plane; Extents Thickness / Distance / Through All / To (click the face or work plane it
ends at) / To Next, Direction Default / Flip / Both. **Unfold** / **Refold**: the whole sheet flat and back (a Cut between them crosses bends).
**Export** (Flat Pattern panel): the flat pattern as DXF (layers OUTER, BEND_UP, BEND_DOWN; lines, arcs and circles as LINE / ARC / CIRCLE). **Flat Pattern**: the view shows the part unfolded with its bend lines (dashed centre lines: up green, down red, each with a note `k UP 90° R2`; bend zones grey) and Flat Pattern Extents Length / Width / Area in Properties (also BOM columns),
Properties its extents and bends; again goes back. **Rip** (Create): click the face, then the rip point
(an edge midpoint or vertex on one of its edges) or the start and end; Face Extents takes a bend's round face.
A converted closed shape (a tube) unfolds only after a rip. **Contour Roll** (Create): click a line of the
open profile, then the axis (a line of the same sketch); Rolled Angle, Side, Direction, Unroll Method.
**Lofted Flange** (Create): click a curve of
each profile's sketch; Die Form or Press Brake with its Facet Control. **Punch Tool** opens its whole panel (selectors take picks, every
option works in the panel) with OK disabled: the interface comes first, the geometry after the owner's check.
**Corner Round** and **Corner Chamfer** (Modify) round or cut the sheet's outside corners through the thickness, in
the folded part and the flat pattern: click corner edges, or drag a box from empty space (left to right: window,
right to left: crossing; Ctrl+box takes them out); Corner Round's *Feature* takes every corner of the clicked
face's feature; sets of corners with their own radius (Add Corner Edge Set); Corner Chamfer: One Distance,
Distance and Angle (click the edge the angle is measured from), Two Distances (one corner, Flip). **Contour
Flange**: Direction Default / Flip / Sym / Asym (Distance 2), Position Default / Flipped / Center.

Aliases (type in the view, no command open): SCV Convert, SMD Defaults, SFA Face, SCF Contour Flange, SMF Flange,
SHM Hem, SFO Fold, SBD Bend, SCR Contour Roll, SLF Lofted Flange, SRP Rip, SCT Cut, SCS Corner Seam, SRD Corner
Round, SCH Corner Chamfer, SPT Punch Tool, SUF Unfold, SRF Refold, SFP Flat Pattern, SFX Export Flat Pattern.
🟡 waiting for a visual check.

### Visual Style and Section View ([spec/0.3-view.md](spec/0.3-view.md) §3.4–3.5)

View › Appearance › **Visual Style**: Shaded (VSS), Shaded with Edges (VSE), Shaded with Hidden Edges (VSH),
Wireframe (VSW), Wireframe with Hidden Edges (VWH), Wireframe with Visible Edges Only (VWV), Monochrome (VSM).
**Section View**: Quarter (QSV), Half (SVH), Three Quarter (TQS), End (NSV) — click a planar face, a work plane or
an origin plane (a face selected before is taken), Offset, Flip Section, OK; the section stays until End Section
View. 🟡 waiting for the owner's check.

### Decisions in fields and Suppress if ([spec/0.5-parameters.md](spec/0.5-parameters.md))

`>` → **If...** in any value field, or the **if** button beside an equation in Parameters: build
`if(condition, value, other)` from lists (parameter · comparison · value, `+ and` / `+ or`, *Type the condition*),
see the result now, Insert. Typing `if(` in a field shows which argument you are in and whether the condition
holds now. Browser → feature → **Suppress if...**: the same condition rows, *Now: true — the feature is
suppressed*, OK / Cancel / Remove; the browser tooltip tells the state now. 🟡 waiting for the owner's check.

### Part commands with their panel only ([spec/2.0-part-commands.md](spec/2.0-part-commands.md))

Every Inventor part command without logic yet opens its whole panel in Properties (tabs, selectors that take
picks in the view, every option with its default); OK and Apply stay disabled. Selectors, Enter, the right-click
menu (Continue / Back / Cancel), the cursor hint and the status prompt work as in Face Draft or Combine. What is
selected before and fits the first selector fills it (rule 17). When the window is too narrow, the rarely used
ribbon panels (Surface, Plastic Part, Freeform, Primitives, Convert, Demo, Format…) collapse first into one
button each that drops the panel down, then the others from the right (Inventor's behaviour).

Aliases: Emboss **RLF**, Derive **DER**, Import **IMP**, Decal **DCL**, Mark **MK**, Unwrap **UNW**,
Thicken/Offset **TO**, Delete Face **DF**, Direct **DE**, Copy Object **CPO**, Move Bodies **MB**, Bend Part **BP**,
Sketch Driven Pattern **SDP**, Stitch **STI**, Sculpt **SCU**, Patch **PAT**, Trim **TRS**, Extend **XS**, Ruled
Surface **RUS**, Replace Face **RF**, Repair Bodies **RB**, Fit Mesh Face **FMF**, Grill **GRL**, Boss **BOS**, Rest
**RST**, Snap Fit **SNF**, Rule Fillet **RUF**, Lip **LIP**, Freeform Box / Plane / Cylinder / Sphere / Torus /
Quadball / Face / Convert **FFB FFP FFY FFS FFT FFQ FFF FFC**, Zebra **ZEB**, Draft Analysis **DFA**, Curvature
**CUR**, Surface Analysis **SAN**, Section Analysis **SEA**, Extract iFeature **XIF**, Create iPart **CIP**, Insert
iFeature **IIF**, Insert Object **INO**, Make Part **MKP**, Make Components **MKC**, Author Frame Profile **AFP**, Customize **CUS**, Studio
**STU**, Stress Analysis **STA**, Shape Generator **SHG**, 3D Print **3DP**, BIM Content **BIM**, Center of Gravity
**COG**, Analysis Visibility **ANV**, Shadows **SHD**, Reflections **RFL**, Ground Plane **GRP**, Ray Tracing **RTR**.
Emboss, Extend and Extract iFeature do not start with E (E must start Extrude at once).
🟡 waiting for the owner's check.

### Drawings (Place Views / Annotate tabs, [spec/4-drawings.md](spec/4-drawings.md))

**File → New… → Drawing** (or File → New Drawing) adds a drawing to the open project and opens it: an A3
ISO sheet with a border and a title block. **BV** = Base View: the Drawing View dialog (Component, Model,
Display Options, Recovery Options); choose the part (or **Browse...** to link a part file), the style and the scale;
the preview sits on the sheet: drag it (or click where it should go), turn it with the ViewCube at its upper
right corner (faces, edges, corners, arrows, roll, drag to orbit; its arrow or a right-click opens the menu with
**Custom View Orientation**: drag anywhere to orbit, Rotate at Angle, Finish Custom View), drag a corner to
scale it (standard scales, **Ctrl** any), click the triangles on its border to add projected views (x removes);
**OK** places it (Edit View has the same cube and corners), then each click beside it places
a projected view (**PV** later; diagonal = isometric); **Esc** or right-click ends. Drag a view to move it
(projected views stay aligned), double-click to edit it, **Delete** removes it with the views projected from
it. Middle button pans, the wheel zooms at the cursor, the sheet's right-click menu has **Zoom All**, Edit
Sheet (**ES**), New Sheet (**NS**). The browser lists the sheets (click activates one) and the views under
their parents. **Annotate**: **D** = Dimension (click an edge, a circle or two points, then where the
dimension goes; above / below the points gives horizontal, beside vertical, a corner aligned), **CM** =
Center Mark, **CL** = Centerline (two circles), **T** = Text (click, type, OK / Ctrl+Enter). Esc or right-click
ends a tool (in a dimension it first drops the picks). Drag a dimension or a note to move its text, Delete
removes the selected one, double-click edits a note. A red **?** marks an annotation whose edge is gone.
**SV** = Section View (the parent; its sketch opens with the Line tool — **L** Line, **D** Dimension, Esc ends the chain / the tool / the sketch, **Enter** or right-click › Finish Sketch; then the side; a section line's right-click › Edit Section Line or a double-click on it opens its sketch again; the toolbar over the sheet has the sketch tools, Project Geometry and Delete — right-click › Floating Toolbar or Application Options › Display turns it off, then the right-click menu and the ribbon's Sketch tab (shown while the sketch is open, opened by itself without the toolbar) have the tools; **Delete** removes the selected curves; drag a point with Select), **DV** = Detail View (the
parent, centre, size — a corner for a rectangular fence —, place; Fence Shape / Cutout Shape / boundary / connection line in the options over the sheet and in Properties; drag the fence's centre grip on the parent to move it, its edge grip to size it), **AV** = Auxiliary View (a straight edge, then the place); **HN** = Hole and
Thread note (a hole's circle, then the place), **LE** = Leader Text (the arrow's tip, the text's place).
**File › Export › PDF...** (or **PDF** in the command search): the PDF Drawing Options, then the file.
**Manage › Styles Editor** in a drawing: the Style and Standard Editor of the drawing (left: Standard and the
style types, bold = in use; right: the style's settings, dimension styles on Inventor's tabs); New / Rename /
Delete at the top; every change redraws the sheet and is one Undo step.
`cargo run --example drawing_demo -- plate.3dcadcraft` builds a plate with holes and its drawing (all view
kinds and annotations) and writes `plate.pdf`.
The other drawing commands open their Inventor dialog with OK disabled (interface first,
[spec/4.7-drawing-commands.md](spec/4.7-drawing-commands.md)); a click on the sheet fills the active selector.
**BO** = Break Out (the selected view or a click on one; its sketch opens with the rectangle tool, Enter / Finish Sketch opens the dialog; click a point of another view for From Point, set Depth, OK; Depth To Hole: click any edge of a hole in any view; right-click the view over it › Edit Break Out / Edit Break Out Sketch / Delete Break Out). Breaks and break outs are listed under their view in the browser (double-click = Edit; right-click › Suppress / Delete). **BRK** = Break (the selected view or a click on one, then the two break lines; the options float over the sheet; drag a break line or its gap to change it; right-click it › Edit Break / Delete Break). Aliases: Overlay **OV**, Draft View **DFV**, Break **BRK**, Break Out **BO**, Slice **SL**, Crop **CR**, Align
**ALH / ALV / ALP / ALB**, New Sketch **DS**, Baseline / Ordinate / Chain **BD / OD / CD** (sets **BDS / ODS /
CDS**), Arrange **AD**, Retrieve **RMA**, Chamfer / Punch / Bend Note **CN / PN / BN**, Symbols **SY**, Surface
Texture **ST**, Welding **WS**, Caterpillar **CAT**, End Fill **EF**, FCF **FCF**, Datum / Feature Identifier
**DI / FI**, Datum Targets **DTL / DTC / DTN / DTP / DTR**, Edge Symbol **EGS**, Transition **TS**, Bisector
**CB**, Centered Pattern **CP**, Automated Centerlines **ACL**, Parts List **PL**, Balloon **B**, Auto Balloon
**AB**, Table **TB**, Hole Tables **HT / HTS / HTF**, Revision Tag / Table / Cloud **RTG / RT / RC**, Edit
Layers **EL**, Update All Sheets **UAS**, Replace Model Reference **RMR**, Define Border / Title Block / Symbol
**DBR / DTB / DSS**. Double-click a dimension: Edit Dimension (Text, Precision and Tolerance, Inspection). Parts List **PL** /
Balloon **B** / Auto Balloon **AB** work (spec 4.4); Drawing View › Sheet Metal View › Flat Pattern and Bend Notes
**BN** work (spec 4.8); Table **TB** makes an empty table or one from a CSV file, Revision Table **RT**, Revision Tag
**RTG** and Revision Cloud **RC** work (spec 4.7 §3.5; double-click edits each); Table **TB** on a flat pattern view makes the bend table (Column Chooser, numeric / alpha
IDs, prefix, title, tags on the bend lines; double-click: the same plus Reverse Direction); double-click a parts list opens its editor (columns, cell
overrides in colour, sort / renumber, hidden and custom rows), a balloon its shape / text; drag moves either.
🟡 waiting for a visual check.

### Help, log and recovery

The **Help** menu (on the right in Quick Access): **Help** (F1), **Save Diagnostics…** (a zip for a problem report, through the program's file dialog), **Open Logs Folder** (`%APPDATA%\3dcadcraft\logs`). The program writes a log and an action journal for every session, autosaves into `%APPDATA%\3dcadcraft\recovery` (every 5 minutes and after OK, never over the user's file) and, on a crash, `crash-….txt`. If the previous session did not close cleanly, **Restore** appears at start-up: "Restore the autosaved version of …?" — **Restore** / **Open Original** / **Discard** 🟡 ([spec/0.14-diagnostics.md](spec/0.14-diagnostics.md)).

### What is remembered between launches

`settings.json` (`%APPDATA%\3dcadcraft\settings.json` on Windows, `~/.config/3dcadcraft/settings.json` on Linux) keeps the **prompt language** (`hints_bulgarian`), the **recent files** (`recent_files`, up to 10), the **layout** (`ui`: visible panels, widths, collapsed ribbon, sides, place and size of the floating windows, size of the main window) and the **Application Options** (`options`: mouse profile, wheel direction, the sketch and inference options, the labels, the thread texture, the Home view, the preview picture saved with a project — Tools → Application Options → Save: *Save preview picture*, isometric or active view, shown in the Open dialog's preview pane with the file's properties). **Cosmetic threads** (Tapped hole, Thread) are shown as a texture of light and dark helical stripes on the face, only along the thread length; Tools → Application Options → Display → *Thread texture* turns it off (🟡 waiting for a visual check). Saving is debounced (0.5 s) and atomic; a corrupt file gives a warning and default values ([spec/0.13-ui-layout.md](spec/0.13-ui-layout.md)). Keys cannot be remapped yet ⬜ (ROADMAP U2).

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
