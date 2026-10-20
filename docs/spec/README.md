# Спецификации на операциите

Един файл за всяка операция или механизъм. Всеки файл описва **какво точно прави** операцията в Autodesk Inventor (функционалния еталон, Inventor 2027) и **докъде сме ние**. Ползва се като:

- **спецификация** преди реализация: всички входове, настройки, ограничения и крайни случаи;
- **чеклист**: статус на всяка точка, ✅ готово · 🟡 частично · ⬜ предстои · ⛔ неприложимо засега (чака друга функция);
- **справка** при съмнение как да се държи програмата.

**Правила:**
- Пише се **със собствени думи**, на български. Не се копира текст, изображения или имена на файлове на Autodesk. Имената на командите и опциите остават на английски, както в интерфейса на Inventor, за лесна миграция.
- Източник: помощта на Inventor (help.autodesk.com, `Inventor-Help`) и обектният модел на API-то (`Inventor-API`), който подсказва какви дефиниции има операцията. Линкът се записва в края.
- Номерът в името на файла сочи раздела в [FEATURES.md](../FEATURES.md).
- Файлът се обновява заедно с кода (правило 9 в `CLAUDE.md`).

**Шаблон:**
1. Предназначение
2. Входни данни (профили, геометрия, избори)
3. Настройки — таблица: опция · поведение в Inventor · ограничения · статус при нас
4. Поведение в дървото на модела и при редакция
5. Крайни случаи и грешки
6. Как е реализирано при нас (модули, имена на топологията)
7. Източници

## Работен процес с една функция

1. **Спецификация:** файлът тук, по помощта на Inventor; всички опции със статус ⬜.
2. **Модел на данните** в правилния слой (`kernel`, `sketch`, `part`, `document`), с тестове: точен брой степени на свобода, 0 излишни ограничения, обем, Ойлер, ориентация, според случая.
3. **Интерфейс** по [0-ui.md](0-ui.md):
   - команда в лентата;
   - подсказка на всяка стъпка;
   - преглед;
   - настройки в Properties;
   - Undo;
   - грешките в лентата на състоянието.
4. **Визуална проверка** в работещото приложение (скриптовете от `CLAUDE.md`), със снимки.
5. **Статусите** в спецификацията, в `FEATURES.md` и в `ROADMAP.md` се обновяват **в същата промяна**. Статус ✅ се слага само след визуална проверка.

## Съдържание

| Файл | Тема |
|---|---|
| [0.3-view.md](0.3-view.md) | Изглед: Orbit през полюсите, ViewCube (Home, компас, влачене, преходи), ос-триада |
| [0.5-parameters.md](0.5-parameters.md) | Параметри, изрази, връзка с размерите и допуските |
| [0.6-files.md](0.6-files.md) | New, Open, Save, Save As, Export, последни файлове, прозорецът за файлове (и под Windows) |
| [0.10-physical.md](0.10-physical.md) | Physical Properties: маса, обем, площ, център на тежестта, инерция |
| [0.9-selection.md](0.9-selection.md) | Избор, филтри, prehighlight, Select Other, контекстни предложения |
| [0.15-context-menus-shortcuts.md](0.15-context-menus-shortcuts.md) | Десен бутон по вид обект и място, marking menu, клавиши и псевдоними, поле за псевдоним, търсене на команда (Alt+S), мишка, улеснения; план на групите |
| [0.9a-selection-filters.md](0.9a-selection-filters.md) | Един модел на избора: всичко + филтри, Select Priority, сечение със селектора на командата, меню Shift + десен клик |
| [0.14-diagnostics.md](0.14-diagnostics.md) | Диагностика: лог на сесията, дневник на действията и replay, автозапис и Restore, доклад за срив, Save Diagnostics |
| [0.15-context-menus-shortcuts.md](0.15-context-menus-shortcuts.md) | Десен бутон, клавиши и псевдоними, мишка, улеснения (Repeat, търсене на команда, подсказка до курсора) |
| [0.11-presets.md](0.11-presets.md) | Presets: готови набори настройки в панелите на командите (Last Used, No Preset, вградени) |
| [0.13-ui-layout.md](0.13-ui-layout.md) | Подредба на интерфейса: размер на панелите, свиване на лентата, User Interface, Clean Screen, Full Screen, плаващи прозорци, `settings.json` |
| [0-ui.md](0-ui.md) | Интерфейс: визия, цветове, знаци, поведение на командите, избор, насочване на потребителя, производителност |
| [1.1-sketch.md](1.1-sketch.md) | 2D скица: създаване, редакция, координатна система, проекции, настройки |
| [1.2-sketch-geometry.md](1.2-sketch-geometry.md) | Инструменти за чертане, Fillet и Chamfer, Format |
| [1.2a-splines.md](1.2a-splines.md) | Сплайни: Interpolation, Control Vertex, Equation, Bridge; дръжки, методи, кривина |
| [1.3-constraints.md](1.3-constraints.md) | Геометрични ограничения, степени на свобода, цветове, Relax Mode, solver |
| [1.4-dimensions.md](1.4-dimensions.md) | Размери: General Dimension, Edit Dimension, driven, показване, толеранси |
| [1.5-inference.md](1.5-inference.md) | Прихващане и автоматични ограничения |
| [1.6-modify.md](1.6-modify.md) | Trim, Extend, Split, Offset, Move, Copy, Rotate, Scale, Stretch |
| [1.7-patterns.md](1.7-patterns.md) | Rectangular, Circular, Mirror |
| [1.8-project-geometry.md](1.8-project-geometry.md) | Проектиране на геометрия |
| [1.9-profiles.md](1.9-profiles.md) | Профили и региони |
| [2.1-extrude.md](2.1-extrude.md) | Extrude, включително консумирани и споделени скици |
| [2.2-revolve.md](2.2-revolve.md) | Revolve, ос и централна линия, диаметрални размери |
| [2.3-hole.md](2.3-hole.md) | Hole: Simple / Clearance / Tapped, гнезда, граници, стандарти в CSV, бележка за чертеж |
| [2.4-fillet.md](2.4-fillet.md) | Fillet (закръгление на ръбове) |
| [2.5-chamfer.md](2.5-chamfer.md) | Chamfer (фаска) |
| [2.8-draft.md](2.8-draft.md) | Face Draft: Fixed Edge, Fixed Plane, посока на изваждане |
| [2.6-shell.md](2.6-shell.md) | Shell (черупка): Remove Faces, Inside / Outside / Both, затворена кухина |
| [2.7-rib.md](2.7-rib.md) | Rib / Web: ребра и прегради от отворени профили, To Next / Finite, Draft, бобини, мрежи |
| [5.3-coil.md](5.3-coil.md) | Coil: пружини и винтови канали, три метода, Taper, затворени краища, резба с Cut |
| [5.1-sweep.md](5.1-sweep.md) | Sweep: профил по път (скица или ръбове), Guide Rail, Parallel, Taper, Twist |
| [5.2-loft.md](5.2-loft.md) | Loft: сечения (скици, стени, точка), Closed Loop, условия, преход |
| [2.11-patterns.md](2.11-patterns.md) | Rectangular Pattern и Circular Pattern на операции и тела |
| [2.12-mirror.md](2.12-mirror.md) | Mirror на операции и тела |
| [2.9-thread.md](2.9-thread.md) | Thread: козметична или моделирана резба, хлабина за 3D печат |
| [2.10-work-features.md](2.10-work-features.md) | Работни равнини, оси, точки |
| [2.13-booleans.md](2.13-booleans.md) | Join / Cut / Intersect, Surface, представяне на резултата |
| [2.13-combine-split.md](2.13-combine-split.md) | Combine (Base, Toolbodies, Keep Toolbody) и Split (Split Face, Trim Solid, Split Solid) |
| [2.15-materials.md](2.15-materials.md) | Материали, външен вид (appearances), override-и, библиотеки, Styles Editor |
| [2.16-browser.md](2.16-browser.md) | Дърво на модела: ред, вложени скици, тела, End of Part, пренареждане, Suppress, Delete, Rename |
| [7.7-export.md](7.7-export.md) | Износ: STL, STEP, GLB |

Тестова матрица на основните функции: [TESTING.md](TESTING.md).
