---
name: cad-ux-review
description: UI/UX review of a 3DCadCraft command, panel or workflow in the running app — Inventor 2027 as the reference, docs/spec/0-ui.md as the rules, Nielsen heuristics and a cognitive walkthrough as the method, screenshots taken through tools/gui_win.py as the evidence. Use after building a command's interface (rule 15), before reporting UI work done (rule 16), or when the owner asks whether something is easy to use or "like Inventor".
---

# CAD UX review (3DCadCraft)

A review that ends in fixes, not only in a report. The generic methods are the two skills next to this
one; this file says how to apply them to a desktop CAD program whose users come from Inventor.

## 1. Who the user is

Default persona (unless the task names another): a mechanical designer with **years of Inventor**, new to
3DCadCraft. They look for Inventor's command names, ribbon places, panel layouts, right-click menus,
aliases and mouse habits, and are slowed down by every difference. A second persona for learnability
checks: a student who knows no CAD program.

## 2. What to look at

1. Read the feature's spec `docs/spec/<section>-<name>.md` (every option row) and `docs/spec/0-ui.md`
   (look §2, command lifecycle §3.1, picking §3.2, guidance §4, language §6).
2. Drive the real app (CLAUDE.md, Visual testing; Windows: `python tools/gui_win.py launch`, clicks,
   `key`, `type`, `shot target/ux-N.png`, crops ×3 for small items). Take one screenshot per step of the
   task — never review from the code alone.
3. Do the task the Inventor way first (ribbon, alias, selection first + right-click menu), then the
   way a novice would try (search Alt+S, hovering tooltips).

## 3. Method

- **Cognitive walkthrough** (`cognitive-walkthrough` skill): split the task into atomic actions
  (start command → each selector → each option → preview → OK / Apply / Cancel → edit again) and answer
  the four questions per action. Q2/Q3 are judged against Inventor: the control must be where and as
  Inventor has it.
- **Heuristic pass** (`nielsen-heuristics-audit` skill) over the same screenshots, with these CAD
  readings of the heuristics:

| Heuristic | What it means here |
|---|---|
| H1 Visibility of status | live preview (rule 12), status-bar prompt for every step, active selector highlighted, "Computing..." for worker jobs, health/sick markers in the browser |
| H2 Real-world match | Inventor's English command and option names, mm / deg units, Inventor glyphs and colours (0-ui §2.2) |
| H3 Control and freedom | Esc cancels without a change, one Undo step per OK, Back in the right-click menu, Edit Feature restores the dialog |
| H4 Consistency | the shared `command_ui` flow, `field_row` / `value_field` for every number (rule 13), same panel order as Inventor |
| H5 Error prevention | OK disabled until inputs are complete, pick filter allows only valid kinds, failure leaves state unchanged (rule 5) |
| H6 Recognition | own icon per command, tooltips with name + alias, selection offers its commands (rule 17) |
| H7 Efficiency | 1–3 letter alias (rule 14), Enter / Space repeat, Ctrl+Enter OK, manipulators in the view, presets row |
| H8 Minimal design | the panel fits the Properties area, the ribbon still fits 1400 px |
| H9 Errors | wording `<Command>: <reason>`, says what to change; no Rust / kernel jargon |
| H10 Help | F1 opens the right page, disabled options show the spec row in the tooltip (rules 7, 15) |

Severity: the 0–4 scale of `nielsen-heuristics-audit`. A difference from Inventor that costs the
persona a search is at least 2; a missing spec option or a model change before OK is 4.

## 4. Output

- Report in **Bulgarian** to the owner, short: what was checked (task, persona, screenshots), a
  table *step · finding · heuristic · severity · fix*, and what already works well.
- Then fix every finding of severity ≥ 2 (or ask the owner where the fix is a design choice), check
  again in the app, and update the spec rows (✅ only after the visual check).
- Do not write a long report file; if a record is needed, findings go into the feature's spec under
  its edge-cases section.
- Never copy Autodesk text or icons while comparing (CLAUDE.md, project paragraph).
