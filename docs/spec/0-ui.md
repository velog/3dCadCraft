# 0. Interface, look, behaviour and guidance

Requirements for the whole application. The screen layout, the mouse, the keys and the glossary for migrating users are in [UI.md](../UI.md). This file describes **how** everything **looks**, **how it behaves** and **how the program guides the user**.

Status: ✅ done · 🟡 partial · ⬜ to do.

## 1. Principles

1. **Inventor's workflow, our own look.** An Inventor user must find the commands in the same place, with the same names and in the same click order. The icons, colours and graphics are our own.
2. **Visible state.** At every moment it is visible which mode the program is in, what is expected as the next step and what a click will do (preview, glyph next to the cursor).
3. **No surprises and no lost work.** A failed operation does not change the model (rule 5 in `CLAUDE.md`). Every change can be undone with Undo. Nothing is deleted or "fixed" silently.
4. **One user step is one Undo step.** Continuous gestures (dragging, spinning a value) are merged into one step.
5. **No modal windows, except where unavoidable.** Settings are in the Properties panel and apply live. Only Edit Dimension and confirmation questions are modal.
6. **Explainability.** Every unavailable command says why. Every error says what is wrong and with what. Every warning is attached to the item that causes it.

## 2. Visual language

### 2.1 General look

| Item | Requirement | Status |
|---|---|---|
| Interface theme | dark panels (ribbon, browser, properties, status bar); a light, neutral view background so the model and the sketch stand out | ✅ |
| Body | blue-grey metal, smooth lighting, dark edges | ✅ |
| Reversed (inner) faces | **red**. This is a visible sign of an orientation bug, never a normal state | ✅ |
| Sketch grid | fine lines in the sketch plane, with a red X axis and a green Y axis; a triad at the origin | ✅ |
| Origin planes | semi-transparent, highlighted when pointed at | ✅ |
| ViewCube | top right; a click on a face gives a standard view; Home | ✅ |
| Anti-aliasing | MSAA 4× where the hardware allows it | ✅ |
| High DPI | the interface follows the system scale | ✅ (egui) |
| Selectable theme (light/dark) and a custom colour scheme | like Inventor's In-canvas Color Scheme | ⬜ |
| **Command icons** | our own vector icons (`gui/icons.rs`), two-coloured (outline + accent, points in orange, removed parts in red); faded for unavailable commands | ✅ |
| **Ribbon** | as in Inventor: large buttons (icon above text), columns of three small buttons (icon + text), icon-only columns for the constraints (the name is in the tooltip); drop-down buttons show the icon of the current variant; toggles (Construction, Relax, DOF) stay pressed while on; all panels of a tab fit in 1400 px | ✅ |
| Model browser icons | project, part, Origin, planes, axes, box, features, sketches | ✅ |

### 2.2 Sketch colours (the meanings are as in Inventor)

| State | Colour | Status |
|---|---|---|
| Under-constrained sketch / curve | **blue** | ✅ |
| Fully constrained sketch | **green** (the whole sketch) | ✅ |
| Fully constrained individual curve | **green** even before the whole sketch is constrained | ✅ |
| Redundant constraint | **orange** | ✅ |
| Conflict, the solver finds no solution | **red**; the sketch does not change | ✅ |
| Projected (reference) geometry | **orange** dashed; its points are orange | ✅ |
| Inactive sketch (not being edited) | grey | ✅ |
| Pointed at / selected / "hot" | **orange** highlight and a thicker line | ✅ |

### 2.3 Line styles

| Kind | Line type | Status |
|---|---|---|
| Normal | solid | ✅ |
| Construction | dashed; never forms a profile | ✅ |
| Centerline | dash-dot; axis for Revolve | ✅ |
| Point | small circle | ✅ |
| Fixed point | solid black square | ✅ |
| Center point (point for holes) | small cross | ⬜ |
| Spline control polygon | thin grey dashed | ✅ |

### 2.4 Glyphs and labels

| Glyph | Meaning | Status |
|---|---|---|
| `H` `V` `//` `90°` `=` `T` `col` `conc` `sym` `mid` `on` `off` | geometric constraints (Horizontal, Vertical, Parallel, Perpendicular, Equal, Tangent, Collinear, Concentric, Symmetric, Midpoint, point on curve, Offset). They are shown next to the geometry; F8 and F9 show and hide them | ✅ |
| Dimension label | value with 2 decimals; `Ø` diameter, `R` radius, `°` angle; driven is **in parentheses** | ✅ |
| Degree-of-freedom glyphs | cross of arrows (free point), double arrow (sliding along a direction), arc arrow (rotation), radial (free radius) | ✅ |
| Glyph next to the cursor while drawing | what will be created: "coincident", "on curve", "midpoint", H, V, "90°", "//" | ✅ |
| Dimension label in Name / Expression / Tolerance mode | shown as name, expression, tolerance or exact value (Value / Name / Expression / Tolerance / Precise Value) | ⬜ |

### 2.5 Previews

| Situation | Look | Status |
|---|---|---|
| Drawing (rubber band) | dashed in the sketch colour | ✅ |
| Trim / Extend / Split | the affected curve in red | ✅ |
| Offset, Mirror, Move, Copy, Rotate, Scale, Stretch, patterns | the future geometry, solid line | ✅ |
| Regions in a sketch | faint blue fill of the closed areas | ✅ |
| Profile selection (Extrude) | the offered regions in pale green, the selected ones in saturated green | ✅ |
| Future 3D body | orange edges, live while the settings change | ✅ |
| Impossible result | the preview disappears and the reason appears in the dialog | ✅ |

### 2.6 Properties panel (modelled on Inventor)

| Requirement | Status |
|---|---|
| Title bar: icon + name of the selection (or of the command) + `?` button (F1) | ✅ (Extrude, Revolve, selected feature) |
| Sections with **collapsible headers** (Input Geometry, Behavior, Output, Advanced Properties…), open/closed state remembered | ✅ |
| Rows in **two columns**: label on the left (aligned), field on the right, equal field widths | 🟡 the feature dialogs ✅; ⬜ the sketch properties |
| Numeric fields show the unit and accept an expression; on error — red frame and tooltip (see 2.7) | ✅ (see 2.7). All fields, incl. Box, the Fillet/Chamfer dialog, the patterns and the tool settings (`valuefield::ExprValue`, visually: Box Length `100/2`, Spacing 45) |
| A choice between variants — **icon buttons** in a group (like Direction in Inventor), not radio buttons | ✅ (Direction, Output, Operation) |
| At the bottom: **OK / Cancel** (and "+" Apply for the next feature where it makes sense) | 🟡 OK / Cancel ✅; ⬜ Apply "+" |
| Warnings — in a frame with the warning colour, above the buttons | ✅ |

### 2.7 Value fields (modelled on Inventor)

Every numeric field — in Properties, in the in-place dimension edit and in the mini-toolbar — is an **expression field**, not just a number field.

| Requirement | Status |
|---|---|
| Accepts an expression: numbers, `+ - * / ^ ( )`, functions, units (`20*2`, `40 mm / 2`, `width + 5`) — the same language as in Parameters ([0.5](0.5-parameters.md)) | ✅ (Extrude, Revolve; dimensions — in the in-place dimension edit) |
| Shows the unit; a bare number gets the field's unit | ✅ |
| Invalid expression — red text/frame and the reason in the tooltip; the value is not applied | 🟡 red text + reason in the tooltip ✅; after Enter the field goes back to the old value (Inventor leaves the red text for correction) |
| An expression with a parameter name keeps the link: the field shows `fx:` and the feature's parameter gets the equation | ✅ |
| A new name in the field (`dist = 20`) creates a parameter with that name | ✅ (unit test; visual — to do) |
| A `>` arrow next to the field opens a menu: **Measure** (measures a distance/angle in the view), **Show Dimensions** (a click on a dimension inserts its name), **Tolerance** (for dimensions), **List Parameters** (list of parameters — a choice inserts the name), **Recently Used** (the last values) | 🟡 List Parameters ✅, Recently Used ✅; 🟡 Measure (`gui/measure_pick.rs`: clicks pick sketch points / curves of the edited sketch or model vertices / edges / faces, two at most; a window lists length, diameter, radius, arc length, area, distance, centre distance, angle — a click on one puts it into the field; Esc / Cancel keeps the field) — also in the sketch's in-place Edit Dimension box, which has its own `>` (List Parameters, Measure) — waiting for a visual check; ⬜ Show Dimensions, Tolerance |
| A click on a displayed dimension in the view, while the field is active, inserts the dimension's name | ⬜ |
| Enter applies, Esc restores the old value | ✅ |

### 2.8 Preview and manipulators — mandatory rule

**Every command for which it is possible has a live preview and a manipulator** — an arrow or an arc that is dragged with the mouse in the view and changes the size (as in Inventor). The value while dragging is shown next to the cursor and is written into the field in Properties; a typed value moves the manipulator. Drag step: 1 mm / 1° / 5° (Revolve angles, as in Inventor); with Ctrl — finer. ⬜ Snapping to vertices and faces (To). The manipulators are visible in the command dialog (the change is live) and when a feature is selected in the browser (applied on release, one Undo step). Dragging replaces the field's equation with a number. Values that may be negative go through zero: a work plane's offset and angle; a one-sided Extrude distance dragged through its profile flips the direction and keeps a positive distance (Inventor); a thickness or two-sided distance stays positive (`Handle::signed`).

| Command | Manipulator | Status |
|---|---|---|
| Extrude | arrow along the normal for Distance A (and a second one for B with Asymmetric); arc for Taper | 🟡 arrows for Distance A/B ✅, grip for Taper ✅ (linear, not an arc); ⬜ for From/To/To Next |
| Revolve | arc about the axis for the angle (a second one with Asymmetric) | ✅ |
| Fillet / Chamfer (3D) | arrow for the radius/distance | ⛔ 2.4, 2.5 |
| Hole | arrow for the depth | ⛔ 2.3 |
| Sketch dimensions | dragging the geometry (already exists) | ✅ |
| Mini-toolbar in the view: the value field + OK/Cancel next to the manipulator | | ⬜ |

### 2.9 Command dialog — separate from Properties

The settings of a command (pattern, Extrude, Revolve, Fillet…) are **not mixed** with the properties of the selection (sketch, feature). While the command is active, the Properties panel shows **only its dialog**, as in Inventor:

| Requirement | Status |
|---|---|
| Title bar: icon, command name, `?` | ✅ (Extrude, Revolve, sketch patterns) |
| **Selectors** for the inputs (Geometry, Direction, Centre, Path, Base, Targets, Profile, Axis…): a row with a name and a state ("Select", "2 curve(s)", "Line"); the active one is coloured and has `>`; clicks in the view go into it | ✅ (patterns; Extrude/Revolve — Profiles/Axis) |
| Once filled, the focus moves automatically to the next empty selector; a click on a row makes it active | ✅ (patterns) |
| What was selected before the command becomes the first input (pre-selection) | ✅ |
| Nothing is created before **OK**; the preview is shown as soon as the input is complete; Enter = next selector / OK; Esc = Cancel | ✅ (patterns); 🟡 Face Draft, Shell (§4.1) |
| Error/impossible result — yellow frame above OK, OK is disabled | ✅ |
| The status bar says what the active selector is waiting for | ✅ |
| All other sketch commands (Move, Copy, Rotate, Mirror, Offset, Fillet…) — with the same model | 🟡 Move, Copy, Rotate, Mirror, Scale, Stretch, Offset, Polygon, Fillet, Chamfer and Edit Pattern have their own dialog (title, the steps as selectors, options, Done); the sketch properties are hidden. ✅ precise values for Move/Copy/Rotate (§2.10) |

### 2.10 Dynamic input (Heads-Up Display)

In Inventor, commands such as Line, Circle and Rectangle show value fields next to the cursor while drawing. You type digits into the field, Tab moves to the next one, and Enter places the point. A typed value locks the geometry and (with "Create dimensions from input values" on) becomes a dimension.

| Behaviour | Inventor | Status |
|---|---|---|
| Line: Length + Angle after the first point | fields next to the cursor; Tab switches between them | ✅ |
| Circle (Center Point): Diameter | one field | ✅ |
| Rectangle (Two Point, Center): Width + Height | two fields | ✅ |
| Typing without clicking into a field | the digits go into the active field | ✅ (digits, `.`, `,`, `-`, Backspace) |
| Tab / Shift+Tab | next / previous field; the typed value is locked | ✅ |
| Enter | places the point with the locked values | ✅ |
| Esc | first clears what was typed, then the command | ✅ |
| Creating dimensions from the input | an option in Application Options | 🟡 always on; the angle becomes an Angle to the previous line of the chain, 0/90° → Horizontal/Vertical; the angle of a standalone line only places it (⬜ dimension to the X axis) |
| X/Y fields for the first point (coordinates) | exist | ⬜ |
| Move / Copy: Delta X, Delta Y after the base point (Inventor: Precise Input) | exists | ✅ (next to the cursor and in the dialog: Values + Apply) |
| Rotate: Angle after the reference point | Angle field in the dialog | ✅ (next to the cursor and in the dialog) |
| X / Y of the first point (Line, Circle, Rectangle, Point, Move/Copy/Rotate) | absolute coordinates in the sketch | ✅ (they only place the point, no dimensions) |
| ↑ in a field: the last value typed into this field | (our convenience) | ✅ |
| Arc, Polygon, Slot | exist for some of them | ⬜ |
| Turning the HUD on/off | Application Options → Sketch | ⬜ |

Sources: [Dynamic input (HUD) in sketches](https://help.autodesk.com/cloudhelp/2014/ENU/Inventor/files/GUID-EFCB58C4-ACF9-4BE1-BAC0-56246BB32F79.htm), [To Draw 2D Geometry Using Dynamic Input](https://help.autodesk.com/cloudhelp/2019/ENU/Inventor-Help/files/GUID-33A8B11D-A04C-4809-BC82-C0419215915F.htm), [Sketch Tab Reference](https://help.autodesk.com/cloudhelp/2018/ENU/Inventor-Help/files/GUID-1898C432-CFD2-46F1-8BAE-631392DFBF1D.htm).

## 3. Behaviour

### 3.1 Command lifecycle

1. **Start:** a ribbon button, a key, a context menu or a double click in the browser. Starting a new command ends the previous one. Starting a 3D command ends the sketch edit.
2. **Prompt:** the status bar says what is expected ("Line: click the next point…"). The prompt changes at every step.
3. **Input:** clicks in the view (with snapping) and values in Properties.
4. **Preview** of the result before confirmation, where possible.
5. **Confirmation:**
   - the drawing tools create the geometry on the last click and **stay active** (as in Inventor);
   - commands with a dialog create the result with **OK**.
6. **Cancel:** **Esc** cancels the current input, and a second Esc returns to Select. Right click → **Done** ends a chain or a spline. **Cancel** closes the dialog without a change.

| Requirement | Status |
|---|---|
| All the steps above for the sketch tools | ✅ |
| Extrude: dialog, preview, OK/Cancel | ✅ |
| Mini-toolbar in the view with values, as in Inventor | ⬜ |
| Manipulators (arrows for dragging a depth or an angle) | 🟡 Extrude and Revolve ✅ (see 2.8) |
| Repeating the last command (Enter or right click → Repeat) | ✅ |
| Marking menu (radial right-click menu; [0.15 §3.6](0.15-context-menus-shortcuts.md)) | 🟡 while a command with a dialog is open: context menu with OK / Continue / Back / Cancel / options (§4.1); radial ⬜ |

### 3.2 Selection and picking priorities

| Rule | Status |
|---|---|
| Click selects, **Shift** adds or removes, **Delete** deletes the selection | ✅ |
| In a sketch the priority is: constraint glyph > point > midpoint of a line (only while drawing) > curve | ✅ |
| In 3D the priority is: **face before origin plane**; when picking a boundary (To/From) **vertex before face** | ✅ |
| Snap radius: 12 px for a point, 8 px for a curve (`overlay::POINT_SNAP_PX`, `CURVE_SNAP_PX`) | ✅ |
| Rectangle selection (window / crossing) | ✅ ([0.9](0.9-selection.md)); Always Window / Crossing modes 🟡 ([0.9a](0.9a-selection-filters.md)) |
| Selection filters (vertices, edges, faces, bodies, sketches, work features, Origin) and intersection with the command's selector | 🟡 ([0.9a](0.9a-selection-filters.md), waiting for a visual check) |
| Selecting a row in Properties highlights the corresponding geometry | ✅ |

### 3.3 In-place editing

- Values in Properties apply immediately and the model regenerates. Dragging a value is one Undo step. ✅
- A double click on a dimension opens Edit Dimension. ✅
- A double click on a sketch in the browser enters the sketch edit. ✅
- A double click on a feature opens its edit. ⬜ (for now through selection and Properties)

### 3.3a View: projection

Inventor (View → Appearance, the ViewCube context menu):

| Mode | Behaviour | Status |
|---|---|---|
| **Orthographic** | parallel projection, no perspective foreshortening (for drawing and measuring) | ✅ |
| **Perspective** | perspective (our previous view) | ✅ |
| **Perspective with Ortho Faces** | perspective, but when the view is exactly along a ViewCube face (Front, Top, …), it switches to orthographic | ✅ |
| Switching: View → Appearance, right click on the ViewCube, the status bar | | 🟡 View → Appearance → Projection ✅, right click in the view → Projection ✅; ⬜ ViewCube menu |
| The mode is remembered (Application Options) | | ⬜ |

### 3.3b ViewCube

| Behaviour (Inventor: *About the ViewCube*, *ViewCube Menu Reference*) | Status |
|---|---|
| 26 directions: 6 faces, 12 edges, 8 corners — the one pointed at is highlighted, a click turns the view there | ✅ |
| When looking straight at a face: 4 arrows to the neighbouring faces | ✅ |
| Two curved arrows: rotating the view by ±90° about the viewing axis | ✅ |
| Home (top left of the cube) | ✅ |
| Right click: Home, Orthographic / Perspective / Perspective with Ortho Faces, **Set Current View as Home**, Reset Home, Fit | ✅ |
| Dragging the cube rotates the view | ⬜ |
| Smooth transition (animation) to the new view | ⬜ |
| The face labels rotate with the cube on roll | ⬜ |

### 3.4 Undo/Redo

- Every change to the model (geometry, constraints, features, visibility, sharing) goes through Undo. ✅
- A failed operation leaves no trace either in the model or in the Undo history (`History::cancel`). ✅
- Undo also returns the selection and the mode to a sensible state (for example the Sketch tab after leaving a sketch). ✅

### 3.5 Update / Rebuild All

In Inventor, **Update** (the Quick Access toolbar) recomputes only the out-of-date features (for example after a dimension changed with deferred update), while **Rebuild All** (Manage → Update) recomputes every feature of the document even if nothing changed. In our program the model is updated right after every change, so Update is not needed.

| Behaviour | Status |
|---|---|
| **Rebuild All**: all features of the part are built again, without the regeneration cache (`part::BuildCache`); **Rebuild** button in the top bar and in Tools → Update | ✅ visually |
| The status bar: "Rebuild All: N features in t s" or how many features failed | ✅ |
| Update (only the out-of-date ones) | ⛔ there is no deferred update |

Source: Autodesk forum, "RE BUILD ALL vs LOCAL UPDATE" (behaviour as described by a community manager).

## 4. Guiding the user (guidance)

| Mechanism | Requirement | Status |
|---|---|---|
| **Status-bar prompt** | every command, at every step, says what to do; without an active command it shows what can be started ("Ready — Start 2D Sketch (S)…") | ✅ |
| **Sketch state** | in the status bar, in the state's colour: "Fully constrained", "Needs N dimension(s)/constraint(s)", redundant, conflict | ✅ |
| **Command tooltips** | every button says what it does and its key | ✅ |
| **Unavailable commands** | grey, with a reason: "Planned — FEATURES X.Y" or "Needs Boolean operations (2.13)" | ✅ |
| **Error messages** | red in the status bar, in the form `<Command>: <reason>` ("Extrude: the radius is too large…"). No technical jargon and no stack trace | ✅ |
| **Warnings ("health")** | "sick" sketches and features are orange in the browser, and the reason is in the tooltip and in Properties | ✅ |
| **Questions on conflict** | a dimension that over-constrains → a question whether it should be driven | ✅ |
| **Explanation in the dialog** | what is disabled and why (for example Join/Cut), what is awaited ("Pick the To face…") | ✅ |
| **Demo sketches** | Sketch tab → Demo sketches panel (rectangle, slot, tangents, lines), for a quick start | ✅ |
| **Help (F1)** | opens the specification of the current command (`docs/spec/…`) with the system program | 🟡 implemented, no visual check; the path points to the source folder (an installation will need the documents packaged) |
| **Command search** | a search field by name (and by the Inventor name), Alt+S — [0.15 §5.3](0.15-context-menus-shortcuts.md) | ⬜ |
| **First launch** | a short introduction: mouse profile, where Start 2D Sketch is | ⬜ |
| **Feedback on long operations** | progress indicator and the option to cancel above ~0.5 s | ⬜ |

### 4.1 Guidance in a command dialog (shared module)

Every 3D command with a dialog guides the user through its inputs in the same way, like
Inventor's panels and dialogs. The logic is in `command::flow` (no GUI, with tests), and
the drawing and events in `gui/command_ui.rs`. Face Draft and Shell already use it.

| Rule | How | Status |
|---|---|---|
| **Selector = row** | name; arrow; **coloured dot** (the same colour as the highlight in the view: faces — blue, direction — orange, plane/tool — turquoise, edges — purple); state ("Select", "3 face(s)", "XY Plane"); `x` clears; red `*` — required and empty | 🟡 Face Draft, Shell |
| **The active selector** | underlined and marked; only it takes clicks in the view; a click on a row makes it active; `x` clears it and makes it active | 🟡 |
| **Automatic advance** | after a valid pick, a single-item selector (direction, plane, tool) moves on to the next empty one (otherwise to the next one); a multi-item selector stays | 🟡 |
| **Right click in the view** | the command's menu instead of the usual one: **OK**, **Apply (+)** (not in Edit Feature), **Continue**, **Back**, **Cancel**, **Repeat <the last one>**, plus the command's options (Draft: Fixed Edge / Fixed Plane / Parting Line, One Way / Symmetric / Asymmetric, Flip pull direction; Shell: Inside / Outside / Center, Sharp / Rounded Corners). OK is disabled while a required input is missing | 🟡 ordinary context menu (radial ⬜) |
| **Continue / Back** | Continue: the first empty required selector, otherwise the next one; Back: the previous one | 🟡 |
| **Enter** | while a required selector is empty — Continue (the focus goes to it); otherwise OK. **Ctrl+Enter** — always OK, if possible. Esc — Cancel. While the dialog is open, Enter/Space do **not** repeat the last command | 🟡 |
| **Prompt next to the cursor** | a yellow label next to the mouse with what the active selector is waiting for ("Select faces to draft", "Select the pull direction (…)") | 🟡 |
| **Status bar** | `<Command>: <the active selector's prompt>`; once everything is selected — "…, or OK (Enter)"; EN + BG (`gui/i18n.rs`) | 🟡 |
| **Prehighlight** | under the cursor only what the active selector accepts is highlighted (`PickKind`: face, planar face, plane, straight edge, edge, axis, sketch, body); for a direction — a faint arrow on the candidate | 🟡 Face Draft |
| **Input glyphs** | the command draws its inputs in the view: the direction (arrow), the plane (outline), the edge that stays (Fixed Edge), the fixed edges | 🟡 Face Draft |

**How another command adopts it** (Extrude, Fillet, Hole, Pattern… — not migrated yet):
1. it describes its selectors as `Vec<Selector>` (key, name, prompt, accepted `PickKind`,
   single/many, required, count, colour), like `command::flow::draft_selectors`;
2. it keeps only the key of the active selector in its draft and builds a `Flow` every frame;
3. in the dialog it draws the rows with `command_ui::selector_rows` and applies `RowEvents`;
4. it is added to `command_ui::Cmd` (`command_flow`, `command_activate`, `command_ok`,
   the menu options); this gives it Enter, the right click, the prompt and the status bar ready-made;
5. picking in the view asks `Flow::accepts(kind)` and calls `picked()` after a valid pick.

## 5. Performance

| Requirement | Status |
|---|---|
| Sketch up to ~200 items: solving and redrawing while dragging under 16 ms | 🟡 (the full Jacobian uses finite differences; splitting into clusters is planned) |
| Sketch regions are computed only on change | ✅ (cache) |
| The Extrude preview is built only when the settings change | ✅ (cache) |
| Regenerating a part with dozens of features under 100 ms | ⬜ (will be measured with a benchmark) |

## 6. Language and accessibility

- Commands are in English, with Inventor's names. Translations come later (UI.md, section 6). 🟡
- Interface texts avoid characters missing from the font (`→`, `⚠` etc., see `CLAUDE.md`). ✅
- Colour is never the only carrier of information: the sketch state is in the text too, the constraints have labels too. ✅
- Full keyboard control. ⬜

## 6a. Sources

- Inventor Help (2026): *ViewCube Menu Reference* — Orthographic / Perspective / Perspective with Ortho Faces — GUID-285B9036-0E97-4F18-A6D2-EF78B8B8FAA9
- Inventor Help (2020): *About the ViewCube* — GUID-B266056E-5E88-42A0-B29C-7BB25C377BD3

## 7. How it is checked

- Every visible function is checked **in the running application** with the scripts from `CLAUDE.md` (`tools/gui_launch.sh`, `tools/gui_sim.py`), with screenshots.
- The colours and glyphs of section 2 are compared on a screenshot after every change in `gui/overlay.rs`.
