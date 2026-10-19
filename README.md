# 3DCadCraft

Параметрична 3D CAD система с отворен код, написана изцяло на Rust: собствено B-Rep ядро, 2D скици с ограничения, един файл на проект и API за AI агенти. Интерфейсът следва Autodesk Inventor за лесна миграция.

*Open-source parametric 3D CAD in pure Rust: clean-room B-Rep kernel, constraint-based sketching, single-file projects, agent-ready API.*

```bash
cargo run --release                                    # стартира приложението
cargo run --release -- my-project.3dcadcraft           # отваря проект
cargo test --no-default-features                       # тестове (без GUI)
```

### Windows

1. Инсталирайте [Git](https://git-scm.com/download/win) и [Rust](https://rustup.rs) (`rustup-init.exe`, с „Desktop development with C++“ от Visual Studio Build Tools, което инсталаторът предлага).
2. В PowerShell:

```powershell
git clone <адрес-на-хранилището> 3dCadCraft
cd 3dCadCraft
cargo run --release
```

Настройките са в `%APPDATA%\3dcadcraft\settings.json`. Скриптовете за визуални тестове в `tools/` (X11) работят само под Linux.

| Документ | |
|---|---|
| [docs/PROJECT.md](docs/PROJECT.md) | визия, принципи, архитектура |
| [docs/FEATURES.md](docs/FEATURES.md) | всички функции по нива 0–7 с подробно описание |
| [docs/UI.md](docs/UI.md) | интерфейс, мишка и клавиши като в Inventor, речник за миграция |
| [docs/FILE_FORMAT.md](docs/FILE_FORMAT.md) | формат `.3dcadcraft`, Link / Append / Pack |
| [docs/ROADMAP.md](docs/ROADMAP.md) | фази, решения на ниво ядро, работни потоци |

Лиценз: MIT OR Apache-2.0. „Inventor“ е търговска марка на Autodesk, Inc. 3DCadCraft не е свързан с Autodesk.
