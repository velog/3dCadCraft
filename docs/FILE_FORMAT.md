# The `.3dcadcraft` file format and the project model

> Status: **implemented, version 1** (`src/document/`). Tests: 11 tests for save/load, links, Pack and Append.

## 1. Requirements

1. **Lossless copying.** A project must be copyable, sendable and archivable as **one file**, without parts of it getting "lost".
2. **Reuse.** Standard parts (bolts, profiles, housings) should be usable in many projects without copying them by hand.
3. **Data safety.** A crash, a power cut or a full disk during a save must not damage the project.
4. **Working with git and with a team.** Changes must show up as differences (diffs), without huge binary files on every change.
5. **Open format.** The file must be readable without 3DCadCraft: a standard container and a documented format.

## 2. Comparison of the approaches

| | Autodesk Inventor | Blender | FreeCAD | Onshape | **3DCadCraft** |
|---|---|---|---|---|---|
| Unit of storage | one file per document (`.ipt`, `.iam`, `.idw`, `.ipn`) + a project file `.ipj` | one `.blend` with many "datablocks" | one `.FCStd` (ZIP) per document | cloud document with tabs | **one `.3dcadcraft` (ZIP) with many documents** |
| References between files | by file path; search along the paths from `.ipj` | **Link** (live link) and **Append** (copy) | App::Link by path | links to a **version** of another document | **Link / Append / Pack** by UUID + relative path |
| Copying a project | risky, needs *Pack and Go* | easy (*Pack Resources*) | easy for one document | not applicable (cloud) | **one file is copied** |
| Identity | file name/path | datablock name | internal names | ID | **UUID of project and document** |
| Backups | `OldVersions/` | `.blend1`, `.blend2` | `.FCBak` | history in the cloud | **atomic save + `.bak`** |
| Working with git | poor (binary files) | poor (binary file) | medium | not applicable | **unpacked folder with JSON** |

### Inventor's problem

In Inventor an assembly (`.iam`) remembers the *paths* to its parts (`.ipt`). If you copy only the assembly or move the folders, you get "Resolve link" dialogs on opening. The remedies are *Pack and Go*, the `.ipj` project files and Vault, but each one adds complexity.

### What Blender does well and where it falls short

Blender keeps everything in one `.blend`, and reuse works through **Link** (a live link to an object in another `.blend`) and **Append** (a copy). *Pack Resources* embeds the external resources. This is very convenient, and it is why we like the model.

It has three weaknesses:

1. References are **by name**, so a rename in the library breaks the link.
2. The file is **binary** and **monolithic**: every save rewrites all of it, and git only sees "the file changed".
3. There is no "document kind". CAD needs a part, an assembly and a drawing with clear roles.

## 3. The advice: yes, Blender's model, with five improvements

**The recommendation is to follow Blender** (one file per project, Link/Append/Pack) with these improvements. All of them are **implemented** in version 1:

1. **UUIDs instead of names.** Every project (`ProjectId`) and document (`DocumentId`) has a globally unique identifier. Renaming and reordering break nothing. On Link we also check that the file found is the *same* project and not just a file with the same name.
2. **Typed documents.** A project contains `Part` and `Assembly` documents. Drawings and presentations are added as new kinds without changing the container.
3. **ZIP with JSON inside.** The format is standard (like `.FCStd`, `.3mf`, `.docx`), opens with any archiver, and every document is a separate entry.
4. **Unpacked form for git.** The same structure can be saved as a **folder** (`Save as folder`). Every document is a separate JSON file, so git shows exactly what changed. `Project::load` opens both forms.
5. **Atomic save.** The data is written to a temporary file, which then replaces the old one with a single rename. The previous version stays as `.bak`.

### When one file is NOT the best choice (honestly)

- **Many people work on one large product.** Inventor+Vault allows "check-out" of individual parts. Our answer is to split the product into **several library projects** (by subsystem), connected with Link, and keep them in git in unpacked form.
- **Huge data** (point clouds, textures, cached meshes). JSON is not suitable for them. The plan is for such data to become **separate binary entries** in the same ZIP, loaded on demand. The format already ignores unknown entries, so this is backwards compatible.

### Next improvement: Link to a version (an idea from Onshape)

Today Link is a "live" link: a change in the library shows up everywhere at once. That is convenient but also dangerous, because someone can change a standard bolt without knowing which projects it affects. I propose that in a later format version `LibraryLink` also stores a **fingerprint (hash) of the version** of the linked document. On a change the user will see "The library has changed — update / keep the old version". This is the equivalent of Inventor's "Update" and Onshape's "linked document versions".

## 4. File structure (version 1)

```text
name.3dcadcraft            ← packed form: ZIP archive (default)
name.3dcadcraft/           ← unpacked form: folder with the same structure
├── mimetype               "application/vnd.3dcadcraft.project+zip" (first, uncompressed)
├── manifest.json          format version, project, document index, libraries
└── documents/
    ├── <uuid>.json        one file per document
    └── …
```

### `manifest.json`

```json
{
  "format": "3dcadcraft",
  "format_version": 1,
  "generator": "3DCadCraft 0.1.0",
  "project":   { "id": "9c35a810-…", "name": "Bracket" },
  "documents": [
    { "id": "97493bf2-…", "name": "Plate", "kind": "Part",     "file": "documents/97493bf2-….json" },
    { "id": "bff8cb0e-…", "name": "Stack", "kind": "Assembly", "file": "documents/bff8cb0e-….json" }
  ],
  "libraries": [
    { "id": "…", "path": "../lib/fasteners.3dcadcraft", "project": "…", "name": "Fasteners" }
  ]
}
```

The index lets tools (a search engine, a file manager, an agent) see the contents without reading every document.

### Document

- **Part:** `sketches` (sketches with a plane, geometry and constraints) and `body` (the B-Rep body). Once the feature tree arrives, it becomes the source of truth and `body` becomes a cache that can be regenerated.
- **Assembly:** `occurrences`. Each one holds a name, `component` (a `DocRef` reference), a transform and a `grounded` flag. A part placed 50 times is stored **once**.

`DocRef { library: None, document }` means a document in the same project. `DocRef { library: Some(id), document }` means a document in a linked library.

## 5. Reuse operations

| Operation | What it does | When to use it |
|---|---|---|
| **Link** (`Resolver::link`) | Adds a reference to a document in another `.3dcadcraft`. Nothing is copied. | Standard/company parts that must be updated centrally. |
| **Append** (`Resolver::append`) | Copies the document **and everything it depends on** (local or from libraries) with new UUIDs. A shared component is copied once. | "Take this subassembly, I will change it my own way." |
| **Pack** (`Resolver::pack`) | Replaces every Link with a local copy and removes the links. The project becomes fully self-contained. | Before sending to a customer/supplier, when archiving an order (≈ *Pack and Go*). |

### How a linked library is found

1. `<project folder>/<stored relative path>` — the normal case. It also works when the whole folder tree is moved.
2. `<project folder>/<library file name>` — when the library was copied next to the project (for example both files were sent by email).

In both cases the `ProjectId` is checked. If a *different* project with the same file name is at that place, the link **refuses** with a clear error instead of silently using the wrong data.

Paths are always stored with `/`, so files move between Linux, Windows and macOS unchanged.

## 6. Save guarantees

| Guarantee | How it is achieved |
|---|---|
| Atomicity | write to a temporary file `.name.tmp-<uuid>`, `fsync`, then `rename` over the target file |
| Backup | the previous version becomes `name.3dcadcraft.bak` (turned off with `SaveOptions { backup: false }`) |
| Determinism | fixed times in the ZIP and a stable order: the same project gives **the same bytes**, i.e. no false "changes" in git/Dropbox |
| Exact numbers | `serde_json` with `float_roundtrip`: coordinates are restored **bit for bit** |
| Versioning | `format_version` and `format_revision` — see §6a |
| Zip slip protection | documents are read only by a name derived from the UUID, never by a path taken from the file |
| Validation on load | unique IDs, every reference points to an existing document, no cycles in assemblies |

## 6a. New program versions and old files

Two numbers in `manifest.json`:

| Number | When it is bumped | What happens on opening |
|---|---|---|
| `format_version` (now 1) | an incompatible change: a renamed or removed field, a different meaning of a value | **older file** → migration in `document::format::load_with` (by number), then it is read; **newer file** → refused with "uses format vN; this build reads up to vM" |
| `format_revision` (now 9) | something added that an older program cannot read (a new kind of fillet, a new enum variant) | the older program opens the file if it can; if it cannot, it reports "made by a newer 3DCadCraft … update the program", not "damaged file" |

A new **optional** field (with `#[serde(default)]`) bumps neither number: old programs skip it, new ones fill in a default. Every change so far was added this way — **no migration has been needed**.

**Test with old files** (`tests/compat.rs`): the folder `tests/compat/r<N>/` keeps projects saved by the program at revision N, and `tests/compat/expected.json` keeps the volume and the numbers of faces, features and sketches at the time they were added. Every `cargo test` opens them, regenerates them and compares. Rules:
- a fixture and its values are **never changed** to make a change pass; if the test fails, old files are breaking → migration;
- with a new revision, files using the new data are added to `r<new>/` and written with `CADCRAFT_WRITE_COMPAT=1 cargo test --test compat` (it writes only the missing ones).

Revision 1 (2026-10-19): Variable / Chord / Full Round fillets, edge sets, equations for the counts in sketch patterns.
Revision 9 (2026-10-19): Face Draft `Parting Line` (a new `DraftType` variant), `extent` Symmetric/Asymmetric with `angle2`, `parting`, Move Parting Line (`move_parting`, `static_edges`, `move_side`), `blending`; Shell `corners` (Rounded). The Center direction of Shell is saved as before — `Both`. An older program would make a one-sided draft or sharp corners, so the revision is bumped; fixture: `tests/compat/r9/draft_r9.3dcadcraft`.

Revision 2 (2026-10-19): partial chamfer (Partial). Caution: an older program does not refuse such a file, it skips the field and makes a full chamfer — so when opening a file with a higher revision the program must warn (see below).
Revision 5 (2026-10-19): Rectangular Pattern, Circular Pattern and Mirror (`Feature::Pattern` with `kind` = `Rectangular` / `Circular` / `Mirror`). File `tests/compat/r5/pattern_r5.3dcadcraft`.
Revision 4 (2026-10-19): the Face Draft feature (`Feature::Draft`).
Revision 3 (2026-10-19): the Shell feature (`Feature::Shell`). An older program cannot read such a file and reports "made by a newer 3DCadCraft … update the program".
Revision 6 (2026-10-19): the Hole features (`Feature::Hole`: `HoleSpec` with the type, seat, termination, drill point, thread and fastener; centres `HoleCenter::Point` from the hole's sketch or `Concentric` to a circular edge) and Thread (`Feature::Thread`: a face + `ThreadSpec`). The numbers from the standards tables are stored in the feature itself, so the file regenerates the same on every machine. Number 5 was taken by a parallel branch. Fixture: `tests/compat/r6/hole_r6.3dcadcraft`.

Revision 7 (2026-10-19): the End of Part marker and suppressed features. The part has two new optional fields: `eop` (how many features are built; missing = all) and `suppressed` (the list of ids of suppressed features). Both are `#[serde(default)]` and are not written when empty, so a file without them stays the same. **The revision is bumped** even though the fields are only additions: an older program would skip them and build *all* features — a different body from the saved one, without a warning. With revision 7 it says "made by a newer 3DCadCraft … update the program". Fixture: `tests/compat/r7/history_r7.3dcadcraft` (a box, two holes, a pattern; the second hole suppressed, the marker above the pattern). Caution when merging: other branches may also have taken number 7 — on the merge the number is renumbered (the next free one) and the fixture moves to the matching `r<N>/` folder.
Revision 12 (2026-10-19): the Sweep features (`Feature::Sweep`: profiles, path `PathRef` — sketch curves or edges, type, rail, orientation, taper, twist) and Loft (`Feature::Loft`: sections `LoftSection` — profile, point, face loop; conditions, transition). Numbers 9–11 were taken by parallel branches; the number may be renumbered on the merge. Fixture: `tests/compat/r12/sweep_loft_r12.3dcadcraft`.
Revision 8 (2026-10-19): new work feature methods (`WorkPlaneDef`: TorusMidplane, TangentEdge, TangentPoint, TangentParallel, NormalToCurve; `WorkAxisDef`: SketchLine, RevolvedFace, SketchCircle; `WorkPointDef`: SketchPoint, SurfaceLine, TorusCenter, SphereCenter), a user coordinate system `WorkDef::Ucs` (`UcsDef::Offsets` / `ThreePoints`) and the field `WorkFeature::flip` (Flip Normal). Fixture: `tests/compat/r8/work_r8.3dcadcraft`.

**Materials and appearances (2026-10-19) — no new revision.** The part got three optional fields: `material` (a name; missing = Generic), `appearance` (a name; missing = As Material) and `looks` with the overrides (`bodies` by body name, `features` by feature id, `faces` by face name + a `hint` point) and the **document copies** of the materials used (`materials`: name, category, density g/cm³, yield strength, Young's modulus, appearance) and appearances (`appearances`: name, category, sRGB colour, opacity, roughness, metalness). All are `#[serde(default)]` and are not written when empty, so a file without materials stays the same byte for byte. `FORMAT_REVISION` **is not bumped**: these are new fields in an existing object, not a new enum variant. An older program skips them and reads the geometry correctly — nothing is misinterpreted. Only the appearance is lost (and the mass is computed as Generic, if it is computed at all). Caution: if an older program saves the file, the fields disappear. That is why no compatibility fixtures were added (`tests/compat/r7/` would make sense only with a new revision). The new fields are checked with a round-trip test (`part::appearance::tests::document_copies_win_and_round_trip`) and with a test that a file without materials contains no new keys.

Revision 10 (2026-10-19): Hole — Flat W / Angle V / Angle Y drill points, Symmetric, Extend To, a centre concentric to a cylindrical face (`HoleCenter::Axis`); point style in a sketch (`PointStyle::Center`, Center Point — an older program reads it as an ordinary point). Fixture `tests/compat/r10/hole_r10.3dcadcraft`.

Revision 11 (2026-10-19): Extrude / Revolve — extents to work / origin planes and points (`TargetRef::Plane`, `TargetRef::Point`), Revolve To (`RevolveExtent::To`); new optional fields `alternate`, `extend_face`, `terminator`, `open` (Match Shape), `minimum`. Fixture `tests/compat/r11/extrude_options_r11.3dcadcraft`.

Revision 12 is reserved for Sweep / Loft (another branch).

**Project Geometry from sketches (2026-10-20) — no new revision.** A sketch in the part got an optional field `links` (projections: `source` = `{"type": "Sketch", "sketch": <sketch id>, "geo": {"kind": "Point", "point": <id>} | {"kind": "Curve", "entity": <id>}}` or `{"type": "Edge", "edge": {faces, hint}}`; `points` — the target points in the source's order; `curve` — the target curve), and the sketch itself got `fixed_radius` (circles with a held radius). Both are `#[serde(default)]` and are not written when empty. There is no new variant of an existing enum, so `FORMAT_REVISION` is not bumped (if it has to be, the next free number is 17 — 15/16 may be taken by parallel branches). An older program skips them: the projected geometry stays as fixed points in place (the link and the held radius are lost), nothing is misinterpreted. Checked by `part::link_tests::round_trip_and_old_files`.

**Loft: Rails, Center Line, Area Loft, conditions, Map Point, Closed Loop (2026-10-20) — no new revision.** All this data was in the model from revision 12 (`rails`, `guide`, `center_line`, `placed`, `conditions` with `angle`/`weight`, `automatic_mapping`, `map_points`, `closed_loop`), only the older program refused it with "… is not supported yet". There are no new fields or variants, so `FORMAT_REVISION` stays 14 (as in main). An older program opens such a file and shows the feature as "sick" with a clear reason — it does not build a different body without a warning. Checked by saving and loading in `part::loft::tests::guided::rail_refusals_leave_the_part_unchanged`.

Revision 13 (2026-10-20): Combine and Split (`Feature::Combine`, `Feature::Split`), the list of hidden bodies `hidden_bodies` in the part. Fixture `tests/compat/r13/combine_split_r13.3dcadcraft`.

**Split with a surface and with open curves (2026-10-20) — no new revision.** `SplitToolRef::Surface(<body name>)` is from revision 13; the surface comes from Extrude / Revolve with `output: "Surface"`, the open curves of Extrude Surface from the existing field `open` (`OpenProfile.curves`). Nothing new in the data, so `FORMAT_REVISION` stays 14 (the next free number after origin/main is 15). An older program opens the file, but builds such a surface with the faceted kernel (there the open curves are closed with a frame, Match Shape), and Split with a surface is "not supported yet" there.

## 7. How to try it

```bash
cargo run --example make_project -- /tmp/bracket.3dcadcraft   # creates a project through the API
unzip -l /tmp/bracket.3dcadcraft                              # look inside
ls /tmp/bracket.src.3dcadcraft/documents                      # the unpacked form
cargo run -- /tmp/bracket.3dcadcraft                          # opens it in the GUI
```

In the GUI the **File** panel has **Save** (one file), **Save as folder** (folder) and **Open** (accepts both).

## 8. Exchange with other programs (not the native format)

The native format keeps the *parametric* model. For exchange there will be separate exporters/importers:

| Format | Direction | Priority | Note |
|---|---|---|---|
| STEP AP242 (`.step`/`.stp`) | import/export | **high** | industry standard for B-Rep; AP242 also carries PMI |
| STL / 3MF | export | high | 3D printing; 3MF keeps units and colours |
| DXF | import/export | medium | 2D sketches and sheet-metal flat patterns |
| PDF | export | medium | drawings |
| IGES | import/export | low | old, but still encountered |
| OBJ / glTF | export | low | visualisation, web |
| `.ipt`/`.iam` | — | no | closed Autodesk format; exchange goes through STEP |
