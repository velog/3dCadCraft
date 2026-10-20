# 3DCadCraft — project description

## Vision

**3DCadCraft** is an open-source parametric 3D CAD system for mechanical designers, written entirely in **Rust**. Its scope is comparable to Autodesk Inventor and SolidWorks: constrained sketches, parametric parts, assemblies, drawings. It runs natively on Linux, Windows and macOS.

Three things set it apart:

1. **Its own, readable kernel.** The B-Rep kernel is written from scratch (clean-room) in safe Rust, with no licensed kernels (ShapeManager, Parasolid) and no C++ dependencies. Anyone can read how a fillet or a flat pattern is computed.
2. **Ready for AI agents from day one.** Every action is a *command* that can be undone and serialized. The same command set is available to the GUI, to scripts and to external agents (JSON-RPC / MCP), without a graphical interface.
3. **One file = a whole project.** A project (`.3dcadcraft`) holds parts and assemblies in one file that can be copied without loss. Libraries are used through Link, Append and Pack, following Blender's model (see [FILE_FORMAT.md](FILE_FORMAT.md)).

## Principles

| Principle | How it is kept |
|---|---|
| **No `unsafe`** | `unsafe_code = "forbid"` for the whole project; FFI to the GPU is isolated in wgpu |
| **Accuracy** | `f64`, millimetres and radians internally, all comparisons through `Tolerance` |
| **Verifiability** | every structure has invariants checked on creation (closed loops, manifold shells, Euler–Poincaré); visual diagnostics (red flipped faces, sketch colours) |
| **Determinism** | the same input gives the same output, including the bytes of the saved file |
| **No graphics by default** | the kernel, the solver, the files and the API compile without the GUI (`--no-default-features`) |
| **Clean-room** | Inventor is the functional reference, but no Autodesk code, documentation or file formats are copied |

## Architecture

```text
 ┌─────────── gui (egui + wgpu) ────────────┐   ┌──── api (JSON-RPC / MCP) ────┐
 │ ribbon, model browser, properties,        │   │ agents, scripts, CI          │
 │ 3D view, ViewCube, sketch overlay         │   │ (no GUI)                     │
 └──────────────────────┬────────────────────┘   └──────────────┬───────────────┘
                        ▼                                       ▼
                command — Command pattern, Undo/Redo, operation tree
                        │
     ┌──────────────────┼────────────────────────┐
     ▼                  ▼                        ▼
  sketch            kernel                    document
  2D entities,      B-Rep geometry             projects, Part/Assembly,
  constraints,      and topology, tessellation .3dcadcraft format,
  solver (LM)       (Booleans, fillets…)       Link / Append / Pack
```

### Modules (`src/`)

| Module | Responsibility | Status |
|---|---|---|
| `kernel` | points, vectors, curves (Line, Circle, NURBS), surfaces (Plane, Cylinder), topology `Vertex…Solid`, validation, tessellation (`mesh.rs`), box primitive | 🟡 the base is done; bodies from sketches and Boolean operations are to do |
| `sketch` | 2D sketch on a plane: points, lines, circles, arcs; 10 constraint kinds; minimum-norm Levenberg solver; analysis of degrees of freedom and redundancies | 🟡 the core is done; drawing tools are to do |
| `part` | part: features → body, sketches with a definition (plane + coordinate system), automatic projection, associativity, Redefine, Edit Coordinate System | 🟡 sketch on a face ✅; Extrude and the other features are to do |
| `document` | project with many documents, UUID identity, ZIP or folder, atomic save, versions, Link/Append/Pack | ✅ version 1 |
| `command` | Undo/Redo (snapshots + gesture coalescing); later a Command enum | 🟡 |
| `api` | JSON-RPC / MCP server | ⬜ |
| `gui` | egui interface in the style of Inventor (see [UI.md](UI.md)), wgpu rendering, ViewCube, sketch overlay and dragging, files | 🟡 |

### Technologies

| Area | Choice | Why |
|---|---|---|
| Language | Rust 2024 (≥ 1.88) | memory safety, performance, one language for everything |
| Linear algebra | `nalgebra` | distinct types for points and vectors, transformations, SVD/Cholesky for the solver |
| Arenas and IDs | `slotmap` | stable typed IDs without pointers or `unsafe` |
| Serialization | `serde` + `serde_json` (`float_roundtrip`) | files and API, exact `f64` |
| Container | `zip` with `zlib-rs` | standard ZIP, entirely in Rust |
| Identity | `uuid` v4 | globally unique IDs of projects and documents |
| GUI | `egui` / `eframe` 0.36 | lightweight immediate-mode UI, above 60 FPS |
| 3D rendering | `wgpu` 30 (via eframe) | Vulkan / Metal / DX12 / GL from one code base |

## How to run it

```bash
cargo run --release                                  # the application
cargo run --release -- path/to/project.3dcadcraft    # open a project
CADCRAFT3D_DEMO=slot cargo run --release             # start with a demo sketch (rectangle|slot|tangents|lines)
cargo test --no-default-features                     # the tests (no GUI)
cargo run --example make_project -- /tmp/b.3dcadcraft # a project created through the API
cargo doc --no-default-features --open               # the code documentation
```

## Documents

| Document | Contents |
|---|---|
| [FEATURES.md](FEATURES.md) | all features by levels 0–7: what they do, how they work, settings, Inventor equivalent, status |
| [UI.md](UI.md) | Inventor-style interface, mouse, keys, glossary for users migrating |
| [FILE_FORMAT.md](FILE_FORMAT.md) | the `.3dcadcraft` format, comparison with Inventor/Blender/FreeCAD/Onshape, recommendation, guarantees |
| [ROADMAP.md](ROADMAP.md) | implementation phases, acceptance criteria, kernel-level decisions, obstacles for assemblies, workflows |
| [../CLAUDE.md](../CLAUDE.md) | rules and recipes for development with Claude (maintained together with the code) |

## Names

| What | Name |
|---|---|
| Product | **3DCadCraft** |
| Program | `3dcadcraft` |
| Rust package and library | `cadcraft3d` (Cargo does not allow a name that starts with a digit) |
| Project files | `*.3dcadcraft` (MIME `application/vnd.3dcadcraft.project+zip`) |

## License

MIT OR Apache-2.0, at the user's option, as is customary in the Rust ecosystem.
