# 3DCadCraft

[![CI](https://github.com/velog/3dCadCraft/actions/workflows/ci.yml/badge.svg)](https://github.com/velog/3dCadCraft/actions/workflows/ci.yml)

**Open-source parametric 3D CAD in pure Rust**, with a user interface that
follows Autodesk Inventor, so Inventor users can switch with little
relearning. Single-file projects, a feature history you can roll back and
edit, and (planned) an API so AI agents can drive it.

> **Status: early, actively developed.** Part modelling works and is tested
> daily, but nothing has been released yet. Expect rough edges — and please
> report them.

*Българска версия — по-долу.*

## What works today

- **Sketching:** lines, arcs, circles, rectangles, slots, polygons, splines;
  ~20 constraint and dimension kinds with a sparse solver and DOF analysis;
  trim/extend/offset/mirror/patterns; projecting geometry from model edges
  and other sketches.
- **Features:** Extrude, Revolve, Hole and Thread (ISO/ANSI tables, cosmetic
  or modeled), Fillet/Chamfer, Shell, Face Draft, Rectangular/Circular
  Pattern and Mirror (thousands of instances), Sweep, Loft (rails, centre
  line, area loft, conditions), Coil, Rib, Combine, Split, work
  planes/axes/points/UCS, primitives.
- **History:** End of Part, reorder, suppress, delete with dependants,
  Edit Feature with live previews and drag handles; expressions and
  parameters in every numeric field.
- **UI:** ribbon, browser, ViewCube, selection filters, command guidance
  (right-click Continue/Back/OK), materials and appearances, STEP/STL/GLB
  export.
- **Diagnostics:** session log, replayable action journal, autosave and
  restore, crash reports.

Geometry runs on an exact B-Rep kernel ([SolidCraft](https://github.com/bherbruck/solidcraft)
on [truck](https://github.com/ricosjp/truck), MIT/Apache), with a few local
fixes kept in `vendor/`.

## Build and run

You need [Rust](https://rustup.rs) (stable). On Windows, also the Visual
Studio C++ build tools (the Rust installer offers them).

```bash
git clone https://github.com/velog/3dCadCraft.git
cd 3dCadCraft
cargo run --release                          # start the application
cargo run --release -- my-part.3dcadcraft    # open a project
cargo test --no-default-features             # tests without the GUI (fast)
cargo xtask ci                               # every check CI runs
```

Use `--release`: geometry is much slower in debug builds. Settings live in
`%APPDATA%\3dcadcraft` (Linux: `~/.config/3dcadcraft`).

## Contributing

Contributions are very welcome — see **[CONTRIBUTING.md](CONTRIBUTING.md)**
and the developer guide **[CLAUDE.md](CLAUDE.md)** (for humans and AI agents
alike). Good places to start: the **Критични** (critical) list at the top of
[docs/ROADMAP.md](docs/ROADMAP.md), and the ⬜ rows in `docs/spec/`.

| Document | |
|---|---|
| [docs/PROJECT.md](docs/PROJECT.md) | vision, principles, architecture |
| [docs/FEATURES.md](docs/FEATURES.md) | every feature by level, with status |
| [docs/spec/](docs/spec/) | per-feature specs: Inventor's behaviour option by option, with our status |
| [docs/UI.md](docs/UI.md), [docs/REFERENCE.md](docs/REFERENCE.md) | interface, mouse, keys, menus |
| [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md) | the `.3dcadcraft` format |
| [docs/ROADMAP.md](docs/ROADMAP.md) | phases and critical issues |

## Licence

Dual-licensed under **MIT** ([LICENSE-MIT](LICENSE-MIT)) **or Apache-2.0**
([LICENSE-APACHE](LICENSE-APACHE)), at your option. Third-party code is
listed in [ATTRIBUTION.md](ATTRIBUTION.md).

"Inventor" and "Autodesk" are trademarks of Autodesk, Inc. 3DCadCraft is an
independent project, not affiliated with or endorsed by Autodesk. It
reproduces Inventor's *behaviour* for familiarity; it contains no Autodesk
code, text, icons or file formats.

---

## На български

Параметрична 3D CAD система с отворен код, написана изцяло на Rust. Интерфейсът следва Autodesk Inventor за лесна миграция; един файл на проект; история на операциите с End of Part; API за AI агенти (предстои).

```powershell
git clone https://github.com/velog/3dCadCraft.git 3dCadCraft
cd 3dCadCraft
cargo run --release
```

Нужни са [Git](https://git-scm.com/download/win) и [Rust](https://rustup.rs) (`rustup-init.exe`, с „Desktop development with C++“ от Visual Studio Build Tools, което инсталаторът предлага). Настройките са в `%APPDATA%\3dcadcraft\settings.json`; скриптовете за визуални тестове в `tools/` (X11) работят само под Linux.

Искате да помогнете? Вижте [CONTRIBUTING.md](CONTRIBUTING.md) и списъка „Критични“ в [docs/ROADMAP.md](docs/ROADMAP.md). При проблем: **Help → Save Diagnostics…** и приложете zip файла към issue.

Лиценз: MIT OR Apache-2.0. „Inventor“ е търговска марка на Autodesk, Inc. 3DCadCraft не е свързан с Autodesk.
