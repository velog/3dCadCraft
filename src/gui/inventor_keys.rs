//! Inventor's keys (spec 0.15): the key combinations it comes with work here
//! too, and Tools → Customize → Keyboard imports the keys and aliases of an
//! installed Inventor from its customization file (the user's own settings).

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::commands::{self, Env};
use super::keymap::Keymap;

/// Inventor's command name, its default key (in `keymap::chord_text`'s
/// spelling) and our command. Commands of several environments are listed once
/// per environment; the one valid in the active environment answers.
pub const DEFAULT_KEYS: &[(&str, &str, &str)] = &[
    ("AppZoomAllCmd", "Home", "view.zoom_all"),
    ("AppZoomSelectCmd", "End", "view.zoom_selected"),
    ("AppLookAtCmd", "PageUp", "view.look_at"),
    ("AppPreviousViewCmd", "F5", "view.previous"),
    ("AppNextViewCmd", "Shift+F5", "view.next"),
    ("AppZoomWindowCmd", "Shift+F3", "view.zoom_window"),
    ("AppParametersCmd", "Alt+Backtick", "tools.parameters"),
    ("AppRebuildAllWrapperCmd", "Alt+B", "tools.rebuild_all"),
    ("AppBodySanityCheckCmd", "Ctrl+F7", "inspect.validate"),
    ("PartWorkPlaneCmd", "CloseBracket", "part.work_plane"),
    ("PartWorkAxisCmd", "Slash", "part.work_axis"),
    ("PartWorkPointCmd", "Period", "part.work_point"),
    (
        "AppOriginPlanesVisibilityCmd",
        "Ctrl+CloseBracket",
        "view.vis_origin_planes",
    ),
    (
        "AppUserWorkPlanesVisibilityCmd",
        "Alt+CloseBracket",
        "view.vis_work_planes",
    ),
    (
        "AppUserWorkAxesVisibilityCmd",
        "Alt+Slash",
        "view.vis_work_axes",
    ),
    (
        "AppUserWorkPointsVisibilityCmd",
        "Alt+Period",
        "view.vis_work_points",
    ),
    (
        "SketchHorizontalConstraintCmd",
        "Minus",
        "sketch.horizontal",
    ),
    ("SketchEqualConstraintCmd", "Equals", "sketch.equal"),
    (
        "SketchCenterPointCircleCmd",
        "Ctrl+Shift+C",
        "sketch.circle_center",
    ),
    ("PartChamferCmd", "Ctrl+Shift+K", "part.chamfer"),
    ("PartFilletCmd", "Ctrl+Shift+F", "part.fillet"),
    ("PartLoftCmd", "Ctrl+Shift+L", "part.loft"),
    (
        "PartCircularPatternCmd",
        "Ctrl+Shift+O",
        "part.circ_pattern",
    ),
    (
        "PartRectangularPatternCmd",
        "Ctrl+Shift+R",
        "part.rect_pattern",
    ),
    ("PartMirrorFeatureCmd", "Ctrl+Shift+M", "part.mirror"),
    (
        "PartCircularPatternCmd",
        "Ctrl+Shift+O",
        "sketch.circ_pattern",
    ),
    (
        "PartRectangularPatternCmd",
        "Ctrl+Shift+R",
        "sketch.rect_pattern",
    ),
    ("PartMirrorFeatureCmd", "Ctrl+Shift+M", "sketch.mirror"),
    ("AssemblyDegreesOfFreedomCmd", "Ctrl+Shift+E", "view.dof"),
    ("AssemblyDegreesOfFreedomCmd", "Ctrl+Shift+E", "sketch.dof"),
    ("AssemblyReplaceCmd", "Ctrl+H", "asm.replace"),
    ("AssembleCmd", "Ctrl+Alt+A", "asm.assemble"),
    ("AppBrowserSearchCmd", "Ctrl+F", "view.browser_search"),
    ("AppFindInBrowserCtxCmd", "Ctrl+B", "view.find_in_browser"),
    ("AssemblyPromoteCmd", "Shift+Tab", "asm.promote"),
    ("AssemblyDemoteCmd", "Tab", "asm.demote"),
    ("AppReturnParentCmd", "Ctrl+Equals", "asm.return_parent"),
    ("AppReturnTopCmd", "Ctrl+Minus", "asm.return_parent"),
    ("DrawingLeaderTextCmd", "Ctrl+Shift+T", "drw.leader_text"),
    ("DrawingInsertSheetCmd", "Ctrl+Shift+N", "drw.new_sheet"),
];

/// Keys an imported file may bring that are not defaults here: Save As keeps
/// Ctrl+Shift+S unless the user's Inventor is imported.
const IMPORT_ONLY_KEYS: &[(&str, &str)] = &[("PartSweepCmd", "part.sweep")];

/// Inventor's commands that have their aliases in the customization file, and
/// ours.
const ALIAS_COMMANDS: &[(&str, &str)] = &[
    ("SketchCoincidentWrapperCmd", "sketch.coincident"),
    ("SketchTwoPointCenRectangleCmd", "sketch.rect_center"),
    ("SketchTwoPointRectangleCmd", "sketch.rect_two"),
    ("SketchLineWrapperCmd", "sketch.line"),
    ("SketchCenterPointCircleCmd", "sketch.circle_center"),
    ("SketchTrimWrapperCmd", "sketch.trim"),
    ("AppDimensionWrapperCmd", "sketch.dimension"),
    ("AssemblyJointComponentCmd", "asm.joint"),
    ("AssemblyPlaceComponentCmd", "asm.place"),
    ("AssemblyCreateComponentCmd", "asm.create"),
    ("AssemblyEndSectionViewCmd", "view.section_end"),
    ("AssemblyHalfSectionViewCmd", "view.section"),
    ("App3DPrintExportCmd", "env.print3d"),
    ("SheetMetalFlatPatternCmd", "sm.flat_pattern"),
    ("SheetMetalCutCmd", "sm.cut"),
    ("PartExtrudeCmd", "part.extrude"),
    ("PartRevolveCmd", "part.revolve"),
    ("PartHoleCmd", "part.hole"),
    ("PartFilletCmd", "part.fillet"),
    ("PartChamferCmd", "part.chamfer"),
    ("PartSweepCmd", "part.sweep"),
    ("PartLoftCmd", "part.loft"),
    ("PartShellCmd", "part.shell"),
];

/// What a customization file holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InventorKeys {
    /// (Inventor command, alias).
    pub aliases: Vec<(String, String)>,
    /// (Inventor command, key in `chord_text`'s spelling).
    pub keys: Vec<(String, String)>,
}

/// The command a key starts by default in `env`, unless the user took the key
/// off that command (`keys[id] == ""` or another key).
pub fn default_command(map: &Keymap, chord: &str, env: Env) -> Option<&'static commands::Command> {
    DEFAULT_KEYS
        .iter()
        .filter(|(_, k, _)| *k == chord)
        .filter(|(_, _, id)| !map.keys.contains_key(*id))
        .find_map(|(_, _, id)| commands::by_id(id).filter(|c| c.valid_in(env) && c.ready()))
}

/// The key named by a Windows virtual key code, as `egui::Key::name` spells it.
fn key_name(vk: u32) -> Option<String> {
    Some(match vk {
        0x30..=0x39 | 0x41..=0x5a => char::from_u32(vk)?.to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6f),
        0x09 => "Tab".into(),
        0x0d => "Enter".into(),
        0x20 => "Space".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0xba => "Semicolon".into(),
        0xbb => "Equals".into(),
        0xbc => "Comma".into(),
        0xbd => "Minus".into(),
        0xbe => "Period".into(),
        0xbf => "Slash".into(),
        0xc0 => "Backtick".into(),
        0xdb => "OpenBracket".into(),
        0xdc => "Backslash".into(),
        0xdd => "CloseBracket".into(),
        _ => return None,
    })
}

/// The key of a stored value `hex:06,00,00,00,<flags>,00,<vk>,00,..`: flag bits
/// 4 Shift, 8 Ctrl, 16 Alt (bit 1 = a virtual key).
fn chord_of_hex(value: &str) -> Option<String> {
    let bytes: Vec<u32> = value
        .strip_prefix("hex:")?
        .split(',')
        .map(|b| u32::from_str_radix(b.trim(), 16))
        .collect::<Result<_, _>>()
        .ok()?;
    let flags = *bytes.get(4)?;
    let name = key_name(*bytes.get(6)?)?;
    let mut s = String::new();
    if flags & 8 != 0 {
        s.push_str("Ctrl+");
    }
    if flags & 16 != 0 {
        s.push_str("Alt+");
    }
    if flags & 4 != 0 {
        s.push_str("Shift+");
    }
    s.push_str(&name);
    Some(s)
}

/// Reads the aliases and keys out of the text of a customization file.
pub fn parse(text: &str) -> InventorKeys {
    let mut out = InventorKeys::default();
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line
            .strip_prefix("<KeyName>\"")
            .and_then(|l| l.strip_suffix("\"</KeyName>"))
        {
            section = if name.starts_with("UI Customization") {
                "aliases"
            } else if name == "Keyboard Shortcuts" {
                "keys"
            } else {
                ""
            };
            continue;
        }
        let Some(value) = line
            .strip_prefix("<Value>\"")
            .and_then(|l| l.strip_suffix("</Value>"))
        else {
            continue;
        };
        let Some((cmd, v)) = value.split_once("\"=") else {
            continue;
        };
        match section {
            "aliases" => {
                if let Some(a) = v.strip_prefix('"').and_then(|a| a.strip_suffix('"')) {
                    out.aliases.push((cmd.to_owned(), a.to_owned()));
                }
            }
            "keys" => {
                if let Some(k) = chord_of_hex(v) {
                    out.keys.push((cmd.to_owned(), k));
                }
            }
            _ => {}
        }
    }
    out
}

/// Decodes a file's bytes: UTF-16 with its byte order mark, else UTF-8.
pub fn decode(bytes: &[u8]) -> String {
    match bytes {
        [0xff, 0xfe, rest @ ..] => String::from_utf16_lossy(
            &rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        ),
        [0xfe, 0xff, rest @ ..] => String::from_utf16_lossy(
            &rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        ),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// The customization file of the newest Inventor installed for this user.
pub fn installed_file() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("APPDATA")?).join("Autodesk");
    let mut years: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("Inventor 20"))
        })
        .map(|p| p.join("InventorCustomization.xml"))
        .filter(|p| p.is_file())
        .collect();
    years.sort();
    years.pop()
}

/// A home in `env` for a command (its alias must be free there).
fn envs_of(c: &commands::Command) -> Vec<Env> {
    [Env::Part, Env::Sketch, Env::Assembly, Env::Drawing]
        .into_iter()
        .filter(|e| c.valid_in(*e))
        .collect()
}

/// Puts what `found` holds into `map`: each known command gets its key and its
/// alias; an alias another command had in that environment leaves it.
/// Returns how many keys and aliases were taken, and the commands of ours that
/// have no counterpart are simply left out.
pub fn apply(map: &mut Keymap, found: &InventorKeys) -> (usize, usize) {
    let our_ids = |inv: &str| -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = DEFAULT_KEYS
            .iter()
            .filter(|(n, _, _)| *n == inv)
            .map(|(_, _, id)| *id)
            .chain(
                IMPORT_ONLY_KEYS
                    .iter()
                    .chain(ALIAS_COMMANDS.iter())
                    .filter(|(n, _)| *n == inv)
                    .map(|(_, id)| *id),
            )
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    };
    let mut keys = 0;
    let mut wanted: BTreeMap<&'static str, String> = BTreeMap::new();
    for (inv, key) in &found.keys {
        for id in our_ids(inv) {
            if commands::by_id(id).is_some() {
                wanted.insert(id, key.clone());
            }
        }
    }
    for (id, key) in wanted {
        let Some(c) = commands::by_id(id) else {
            continue;
        };
        let is_default = DEFAULT_KEYS
            .iter()
            .any(|(_, k, d)| *d == id && *k == key.as_str());
        if !is_default {
            map.set_key(c, Some(key));
        }
        keys += 1;
    }
    let mut aliases = 0;
    for (inv, alias) in &found.aliases {
        for id in our_ids(inv) {
            let Some(c) = commands::by_id(id) else {
                continue;
            };
            for env in envs_of(c) {
                if let Some(other) = map.alias_owner(alias, env, id) {
                    let rest: Vec<String> = map
                        .aliases_of(other)
                        .into_iter()
                        .filter(|a| !a.eq_ignore_ascii_case(alias))
                        .collect();
                    map.set_aliases(other, &rest.join(" "));
                }
            }
            let mut list = map.aliases_of(c);
            if !list.iter().any(|a| a.eq_ignore_ascii_case(alias)) {
                list.insert(0, alias.clone());
                map.set_aliases(c, &list.join(" "));
            }
            aliases += 1;
        }
    }
    (keys, aliases)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<KeyName>"UI Customization V6"</KeyName>
      <KeyContents>
        <Value>"UseDefaultMultiCharCmdAlias"=dword:0x00000001</Value>
        <Value>"SketchCoincidentWrapperCmd"="C"</Value>
        <Value>"AssemblyJointComponentCmd"="J"</Value>
      </KeyContents>
      <KeyName>"Keyboard Shortcuts"</KeyName>
      <KeyContents>
        <Value>"AppPreviousViewCmd"=hex:06,00,00,00,01,00,74,00,b4,60</Value>
        <Value>"AppNextViewCmd"=hex:06,00,00,00,05,00,74,00,98,60</Value>
        <Value>"PartChamferCmd"=hex:06,00,00,00,0d,00,4b,00,10,20</Value>
        <Value>"AppParametersCmd"=hex:06,00,00,00,11,00,c0,00,c9,71</Value>
      </KeyContents>"#;

    #[test]
    fn reads_aliases_and_keys() {
        let k = parse(SAMPLE);
        assert_eq!(
            k.aliases,
            vec![
                ("SketchCoincidentWrapperCmd".to_owned(), "C".to_owned()),
                ("AssemblyJointComponentCmd".to_owned(), "J".to_owned())
            ]
        );
        assert!(
            k.keys
                .contains(&("AppPreviousViewCmd".to_owned(), "F5".to_owned()))
        );
        assert!(
            k.keys
                .contains(&("AppNextViewCmd".to_owned(), "Shift+F5".to_owned()))
        );
        assert!(
            k.keys
                .contains(&("PartChamferCmd".to_owned(), "Ctrl+Shift+K".to_owned()))
        );
        assert!(
            k.keys
                .contains(&("AppParametersCmd".to_owned(), "Alt+Backtick".to_owned()))
        );
    }

    #[test]
    fn utf16_with_a_mark_is_decoded() {
        let mut b = vec![0xff, 0xfe];
        for u in "<a>".encode_utf16() {
            b.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode(&b), "<a>");
    }

    #[test]
    fn every_default_key_names_a_command() {
        for (inv, key, id) in DEFAULT_KEYS {
            assert!(commands::by_id(id).is_some(), "{inv} {key} {id}");
        }
    }

    #[test]
    fn default_keys_answer_only_in_their_environment() {
        let map = Keymap::default();
        let f = default_command(&map, "Ctrl+Shift+R", Env::Part).map(|c| c.id);
        assert_eq!(f, Some("part.rect_pattern"));
        let f = default_command(&map, "Ctrl+Shift+R", Env::Sketch).map(|c| c.id);
        assert_eq!(f, Some("sketch.rect_pattern"));
        assert!(default_command(&map, "Ctrl+Shift+K", Env::Drawing).is_none());
    }

    #[test]
    fn an_imported_alias_leaves_the_command_that_had_it() {
        let mut map = Keymap::default();
        let found = parse(SAMPLE);
        let (_, aliases) = apply(&mut map, &found);
        assert_eq!(aliases, 2);
        let m = commands::alias_match_in("C", Env::Sketch, &map);
        assert_eq!(m.exact.map(|c| c.id), Some("sketch.coincident"));
        let circle = commands::by_id("sketch.circle_center");
        assert!(circle.is_some_and(|c| !map.aliases_of(c).iter().any(|a| a == "C")));
    }
}
