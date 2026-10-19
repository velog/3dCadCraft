//! Hints in Bulgarian (status-bar prompts and the "Ready" line). Command
//! and option names stay in English, as in Inventor, so that its help and
//! tutorials still match; only the sentences around them are translated.
//!
//! The choice is kept in `settings.json` (`~/.config/3dcadcraft`, or
//! `%APPDATA%\\3dcadcraft` on Windows).

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};

static BULGARIAN: AtomicBool = AtomicBool::new(false);

/// Whether hints are shown in Bulgarian.
pub fn bulgarian() -> bool {
    BULGARIAN.load(Ordering::Relaxed)
}

/// Switches the hint language and remembers it.
pub fn set_bulgarian(on: bool) {
    BULGARIAN.store(on, Ordering::Relaxed);
    save();
}

fn settings_path() -> Option<std::path::PathBuf> {
    super::files::settings_path()
}

/// Loads the remembered choice (at start-up).
pub fn load() {
    let on = settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("hints_bulgarian")?.as_bool())
        .unwrap_or(false);
    BULGARIAN.store(on, Ordering::Relaxed);
}

fn save() {
    let Some(p) = settings_path() else { return };
    let mut v = std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    v["hints_bulgarian"] = serde_json::Value::Bool(bulgarian());
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(&v) {
        let _ = std::fs::write(&p, text);
    }
}

/// `s` in the hint language. Unknown sentences stay in English.
pub fn tr(s: &str) -> Cow<'_, str> {
    if !bulgarian() {
        return Cow::Borrowed(s);
    }
    match TABLE.iter().find(|(en, _)| *en == s) {
        Some((_, bg)) => Cow::Borrowed(bg),
        None => Cow::Borrowed(s),
    }
}

/// English → Bulgarian. Command names (before the colon) stay English.
const TABLE: &[(&str, &str)] = &[
    (
        "Face Draft: click a planar face, an origin plane or a straight edge for the pull direction",
        "Face Draft: щракнете плоска стена, основна равнина или прав ръб за посоката на изваждане",
    ),
    (
        "Face Draft: click the fixed plane (a planar face or an origin plane)",
        "Face Draft: щракнете неподвижната равнина (плоска стена или основна равнина)",
    ),
    (
        "Face Draft: click the faces to draft (Fixed Edge: near the edge that stays), then OK",
        "Face Draft: щракнете стените за накланяне (Fixed Edge: близо до ръба, който остава), после OK",
    ),
    (
        "Shell: click the faces to remove (none: a closed cavity), set the thickness, then OK",
        "Shell: щракнете стените за махане (без стени: затворена кухина), задайте дебелината, после OK",
    ),
    (
        "Shell: click more faces to remove (a second click puts one back), then OK",
        "Shell: щракнете още стени за махане (втори клик връща стената), после OK",
    ),
    (
        "Ready — Start 2D Sketch (S), or right-click a face: New Sketch",
        "Готово — Start 2D Sketch (S), или десен клик върху стена: New Sketch",
    ),
    (
        "Select a planar face or an origin plane to sketch on (Esc cancels)",
        "Изберете плоска стена или равнина на Origin за скицата (Esc отказва)",
    ),
    (
        "Select geometry, drag points; Delete removes the selection",
        "Изберете геометрия, влачете точки; Delete изтрива избраното",
    ),
    (
        "Line: click the start point (drag from a curve end for a tangent/perpendicular arc)",
        "Line: щракнете началната точка (влачене от край на крива дава допирателна/перпендикулярна дъга)",
    ),
    (
        "Line: click the next point (Esc or right-click ends; Ctrl = no inference)",
        "Line: щракнете следващата точка (Esc или десен клик завършва; Ctrl = без разпознаване)",
    ),
    (
        "Center Point Circle: click the centre",
        "Center Point Circle: щракнете центъра",
    ),
    (
        "Center Point Circle: click a point on the circle (on a curve = tangent)",
        "Center Point Circle: щракнете точка от окръжността (върху крива = допирателна)",
    ),
    (
        "Tangent Circle: select three lines",
        "Tangent Circle: изберете три линии",
    ),
    (
        "Three Point Arc: click the two end points",
        "Three Point Arc: щракнете двата края",
    ),
    (
        "Three Point Arc: click a point on the arc (sets side and radius)",
        "Three Point Arc: щракнете точка от дъгата (страна и радиус)",
    ),
    (
        "Tangent Arc: click the end of a line or arc",
        "Tangent Arc: щракнете края на линия или дъга",
    ),
    (
        "Tangent Arc: click the arc end point",
        "Tangent Arc: щракнете края на дъгата",
    ),
    (
        "Center Point Arc: click the centre",
        "Center Point Arc: щракнете центъра",
    ),
    (
        "Center Point Arc: click the start point (radius)",
        "Center Point Arc: щракнете началната точка (радиус)",
    ),
    (
        "Center Point Arc: click the end point",
        "Center Point Arc: щракнете крайната точка",
    ),
    (
        "Two Point Rectangle: click two opposite corners",
        "Two Point Rectangle: щракнете два срещуположни ъгъла",
    ),
    (
        "Three Point Rectangle: click a corner, then the end of the first side",
        "Three Point Rectangle: щракнете ъгъл, после края на първата страна",
    ),
    (
        "Three Point Rectangle: click to set the adjacent side",
        "Three Point Rectangle: щракнете за съседната страна",
    ),
    (
        "Center Rectangle: click the centre, then a corner",
        "Center Rectangle: щракнете центъра, после ъгъл",
    ),
    (
        "Center Rectangle: click the centre, then half a side",
        "Center Rectangle: щракнете центъра, после половин страна",
    ),
    (
        "Center Rectangle: click to set the adjacent side",
        "Center Rectangle: щракнете за съседната страна",
    ),
    (
        "Center to Center Slot: click both arc centres",
        "Center to Center Slot: щракнете двата центъра на дъгите",
    ),
    (
        "Overall Slot: click both slot ends",
        "Overall Slot: щракнете двата края на слота",
    ),
    (
        "Center Point Slot: click the slot centre, then an arc centre",
        "Center Point Slot: щракнете центъра на слота, после център на дъга",
    ),
    (
        "Three Point Arc Slot: click start, end, then a point on the arc",
        "Three Point Arc Slot: щракнете начало, край, после точка от дъгата",
    ),
    (
        "Center Point Arc Slot: click the centre, start, then end",
        "Center Point Arc Slot: щракнете център, начало, после край",
    ),
    ("Slot: click to set the width", "Slot: щракнете за ширината"),
    (
        "Polygon: click the centre, then a vertex (inscribed) or side midpoint (circumscribed)",
        "Polygon: щракнете центъра, после връх (вписан) или среда на страна (описан)",
    ),
    (
        "Spline: click control points; Enter or double-click finishes",
        "Spline: щракнете контролните точки; Enter или двоен клик завършва",
    ),
    (
        "Interpolation Spline: click the points it passes through; Enter or double-click finishes",
        "Interpolation Spline: щракнете точките, през които минава; Enter или двоен клик завършва",
    ),
    ("Point: click to place", "Point: щракнете, за да поставите"),
    (
        "Dimension: select a line, circle, arc or point",
        "Dimension: изберете линия, окръжност, дъга или точка",
    ),
    (
        "Dimension: select more geometry, or click empty space to place",
        "Dimension: изберете още геометрия или щракнете на празно място, за да поставите",
    ),
    (
        "Trim: click the part of a curve to remove (Shift = Extend)",
        "Trim: щракнете частта от кривата за премахване (Shift = Extend)",
    ),
    (
        "Extend: click near the end to extend (Shift = Trim)",
        "Extend: щракнете близо до края за удължаване (Shift = Trim)",
    ),
    (
        "Split: click a curve near where it should be split",
        "Split: щракнете кривата там, където да се раздели",
    ),
    (
        "Offset: select a curve (Loop Select / Constrain Offset in Properties)",
        "Offset: изберете крива (Loop Select / Constrain Offset в Properties)",
    ),
    (
        "Offset: click to set the offset distance",
        "Offset: щракнете за отстоянието",
    ),
    (
        "Select the first line, or click a corner (size in Properties)",
        "Изберете първата линия или щракнете ъгъл (размерът е в Properties)",
    ),
    ("Select the second line", "Изберете втората линия"),
    ("Select the first line", "Изберете първата линия"),
    (
        "Select geometry, then press Enter (or right-click: Done)",
        "Изберете геометрия, после Enter (или десен клик: Done)",
    ),
    (
        "Scale: click the base point (scale factor in Properties)",
        "Scale: щракнете базовата точка (мащабът е в Properties)",
    ),
    (
        "Stretch: click the base point",
        "Stretch: щракнете базовата точка",
    ),
    (
        "Stretch: click the destination point",
        "Stretch: щракнете целевата точка",
    ),
    ("Click the base point", "Щракнете базовата точка"),
    ("Click the destination point", "Щракнете целевата точка"),
    ("Rotate: click the centre point", "Rotate: щракнете центъра"),
    (
        "Rotate: click a reference point",
        "Rotate: щракнете опорна точка",
    ),
    (
        "Rotate: click to set the angle",
        "Rotate: щракнете за ъгъла",
    ),
    (
        "Mirror: click the mirror line",
        "Mirror: щракнете линията на огледалото",
    ),
    (
        "Pattern at Points: click the base point of the selected geometry",
        "Pattern at Points: щракнете базовата точка на избраната геометрия",
    ),
    (
        "Pattern at Points: click target points (empty space adds a point); Enter: finish",
        "Pattern at Points: щракнете целевите точки (празно място добавя точка); Enter: край",
    ),
    ("Coincident: select a point", "Coincident: изберете точка"),
    (
        "Coincident: select a point or curve to constrain it to",
        "Coincident: изберете точка или крива, към която да се свърже",
    ),
    (
        "Concentric: select the first circle or arc",
        "Concentric: изберете първата окръжност или дъга",
    ),
    (
        "Concentric: select the second circle or arc",
        "Concentric: изберете втората окръжност или дъга",
    ),
    (
        "Fix: select a point or curve",
        "Fix: изберете точка или крива",
    ),
    (
        "Select a line, or a first point",
        "Изберете линия или първа точка",
    ),
    ("Select the second point", "Изберете втората точка"),
    (
        "Tangent: select the first curve",
        "Tangent: изберете първата крива",
    ),
    (
        "Tangent: select the second curve",
        "Tangent: изберете втората крива",
    ),
    (
        "Symmetric: select the first point or curve",
        "Symmetric: изберете първата точка или крива",
    ),
    (
        "Symmetric: select the second point or curve",
        "Symmetric: изберете втората точка или крива",
    ),
    (
        "Symmetric: select the symmetry line",
        "Symmetric: изберете оста на симетрия",
    ),
    (
        "Equal: select a line, circle or arc",
        "Equal: изберете линия, окръжност или дъга",
    ),
    (
        "Equal: select another of the same kind",
        "Equal: изберете друга от същия вид",
    ),
    (
        "Select the geometry to pattern (click curves; Enter: next)",
        "Изберете геометрията за масива (щракнете криви; Enter: напред)",
    ),
    (
        "Direction 1: click a line (or X / Y in the dialog)",
        "Direction 1: щракнете линия (или X / Y в диалога)",
    ),
    (
        "Direction 2 (optional): click a line, or OK",
        "Direction 2 (по избор): щракнете линия или OK",
    ),
    (
        "Centre: click a point (or 'By radius' in the dialog)",
        "Център: щракнете точка (или 'By radius' в диалога)",
    ),
    (
        "Path: click a line, arc or spline",
        "Path: щракнете линия, дъга или сплайн",
    ),
    (
        "Base point: click a point of the geometry",
        "Базова точка: щракнете точка от геометрията",
    ),
    (
        "Targets: click points (empty space adds one); OK when done",
        "Цели: щракнете точки (празно място добавя); OK накрая",
    ),
    (
        "Shift+Middle / F4: orbit · Middle / F2: pan · Wheel / F3: zoom · F6: home",
        "Shift+среден / F4: завъртане · среден / F2: местене · колелце / F3: мащаб · F6: начало",
    ),
    (
        "Left: orbit · Right/Middle: pan · Wheel: zoom · Double-click: fit",
        "Ляв: завъртане · десен/среден: местене · колелце: мащаб · двоен клик: всичко",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_no_duplicates_and_keeps_command_names() {
        for (i, (en, bg)) in TABLE.iter().enumerate() {
            assert!(
                TABLE[i + 1..].iter().all(|(e, _)| e != en),
                "duplicate: {en}"
            );
            // "Command: …" keeps its English command name.
            if let Some((cmd, _)) = en.split_once(": ")
                && cmd.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ')
                && cmd.chars().next().is_some_and(char::is_uppercase)
                && !matches!(cmd, "Centre" | "Base point" | "Targets" | "Left")
            {
                assert!(bg.starts_with(cmd), "{en} -> {bg}");
            }
        }
    }
}
