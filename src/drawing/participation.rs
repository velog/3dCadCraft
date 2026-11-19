//! Section participation: which components a section view cuts
//! (`docs/spec/4.15-section-participation.md`). GUI-free: the GUI gathers
//! each part's settings into [`Leaf`]s, [`uncut`] gives the instance
//! prefixes [`super::hlr::Aligned::uncut`] leaves whole.

use crate::document::iprops::SectionParticipation;
use serde::{Deserialize, Serialize};

/// The view option *Section Standard Parts* (Drawing View › Display).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StandardSection {
    /// The components' own settings decide (the default).
    #[default]
    ObeyBrowser,
    /// Standard parts are never cut.
    Never,
    /// Standard parts are always cut.
    Always,
}

impl StandardSection {
    /// The default (skipped in files).
    pub fn is_default(&self) -> bool {
        *self == Self::ObeyBrowser
    }

    /// Every choice, in Inventor's order.
    pub const ALL: [Self; 3] = [Self::ObeyBrowser, Self::Never, Self::Always];

    /// Its name in the dialog.
    pub fn label(self) -> &'static str {
        match self {
            Self::ObeyBrowser => "Obey Browser",
            Self::Never => "Never",
            Self::Always => "Always",
        }
    }
}

/// A view's own choice for one component: the occurrence path from the top
/// assembly (`occurrence`, then `sub` down to a part or a subassembly).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticipationOverride {
    /// Top-level occurrence id.
    pub occurrence: u32,
    /// The path below it (empty: the occurrence itself).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sub: Vec<u32>,
    /// Section or None (Default removes the entry).
    pub value: SectionParticipation,
}

/// One part of a section view's model with what decides whether it is cut.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Leaf {
    /// Top-level occurrence id.
    pub occurrence: u32,
    /// The path below it to the part.
    pub sub: Vec<u32>,
    /// The occurrences' own settings from the part's up to the top-level
    /// one (innermost first).
    pub occurrences: Vec<SectionParticipation>,
    /// The part's Properties.
    pub part: SectionParticipation,
    /// A standard part (library member, generated shaft): not cut by
    /// default.
    pub standard: bool,
}

/// The view's override of `leaf`: its own path or the nearest above it.
pub fn view_override(overrides: &[ParticipationOverride], leaf: &Leaf) -> Option<bool> {
    (0..=leaf.sub.len()).rev().find_map(|n| {
        overrides
            .iter()
            .find(|o| o.occurrence == leaf.occurrence && o.sub == leaf.sub[..n])
            .and_then(|o| o.value.cut())
    })
}

/// Whether the section cuts `leaf` (precedence of spec 4.15 §3) and which
/// setting decided it.
pub fn decide(
    overrides: &[ParticipationOverride],
    standard: StandardSection,
    leaf: &Leaf,
) -> (bool, Source) {
    if let Some(c) = view_override(overrides, leaf) {
        return (c, Source::View);
    }
    if leaf.standard {
        match standard {
            StandardSection::Never => return (false, Source::ViewOption),
            StandardSection::Always => return (true, Source::ViewOption),
            StandardSection::ObeyBrowser => {}
        }
    }
    if let Some(c) = leaf.occurrences.iter().find_map(|o| o.cut()) {
        return (c, Source::Occurrence);
    }
    if let Some(c) = leaf.part.cut() {
        return (c, Source::Part);
    }
    (!leaf.standard, Source::Category)
}

/// Whether the section cuts `leaf`.
pub fn cut(overrides: &[ParticipationOverride], standard: StandardSection, leaf: &Leaf) -> bool {
    decide(overrides, standard, leaf).0
}

/// Which setting decided a component's participation (browser tags).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The view's own override.
    View,
    /// The view's Section Standard Parts.
    ViewOption,
    /// An assembly occurrence.
    Occurrence,
    /// The part's Properties.
    Part,
    /// The category default.
    Category,
}

/// The prefix of a part instance's edge and face names in an assembly view
/// (`o<id>/` or `o<id>/s<a>.<b>/`, as [`super::hlr::assembly_model`] names
/// them).
pub fn prefix(occurrence: u32, sub: &[u32]) -> String {
    if sub.is_empty() {
        return format!("o{occurrence}/");
    }
    let sub: Vec<String> = sub.iter().map(u32::to_string).collect();
    format!("o{occurrence}/s{}/", sub.join("."))
}

/// The instance prefix a name of an assembly view starts with.
pub fn prefix_of(name: &str) -> Option<&str> {
    let rest = name.strip_prefix('o')?;
    let i = rest.find('/')?;
    let after = &rest[i + 1..];
    let end = match after.strip_prefix('s') {
        Some(s) if s.starts_with(|c: char| c.is_ascii_digit()) => i + 2 + 1 + s.find('/')? + 1,
        _ => i + 2,
    };
    name.get(..end)
}

/// The prefixes of the leaves a section leaves whole (sorted: the view's
/// cache key).
pub fn uncut(
    overrides: &[ParticipationOverride],
    standard: StandardSection,
    leaves: &[Leaf],
) -> Vec<String> {
    let mut out: Vec<String> = leaves
        .iter()
        .filter(|l| !cut(overrides, standard, l))
        .map(|l| prefix(l.occurrence, &l.sub))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Sets the view's choice for the component at (`occurrence`, `sub`):
/// Default removes it.
pub fn set_override(
    overrides: &mut Vec<ParticipationOverride>,
    occurrence: u32,
    sub: &[u32],
    value: SectionParticipation,
) {
    overrides.retain(|o| !(o.occurrence == occurrence && o.sub == sub));
    if !value.is_default() {
        overrides.push(ParticipationOverride {
            occurrence,
            sub: sub.to_vec(),
            value,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SectionParticipation as P;

    fn leaf(standard: bool) -> Leaf {
        Leaf {
            occurrence: 3,
            sub: vec![1, 2],
            occurrences: vec![P::Default, P::Default],
            part: P::Default,
            standard,
        }
    }

    #[test]
    fn precedence_view_option_occurrence_part_category() {
        let none: &[ParticipationOverride] = &[];
        let ob = StandardSection::ObeyBrowser;
        assert!(cut(none, ob, &leaf(false)));
        assert!(!cut(none, ob, &leaf(true)), "standard parts stay whole");
        let mut l = leaf(true);
        l.part = P::Section;
        assert_eq!(decide(none, ob, &l), (true, Source::Part));
        l.occurrences[1] = P::None; // the subassembly above
        assert_eq!(decide(none, ob, &l), (false, Source::Occurrence));
        l.occurrences[0] = P::Section; // the part's own occurrence wins
        assert!(cut(none, ob, &l));
        assert_eq!(
            decide(none, StandardSection::Never, &l),
            (false, Source::ViewOption)
        );
        assert!(
            cut(none, StandardSection::Never, &leaf(false)),
            "only standard parts"
        );
        let mut ov = Vec::new();
        set_override(&mut ov, 3, &[1], P::Section); // a subassembly
        assert_eq!(
            decide(&ov, StandardSection::Never, &leaf(true)),
            (true, Source::View)
        );
        set_override(&mut ov, 3, &[1, 2], P::None); // the part itself wins
        assert!(!cut(&ov, StandardSection::Always, &leaf(false)));
        set_override(&mut ov, 3, &[1, 2], P::Default);
        assert_eq!(ov.len(), 1);
    }

    #[test]
    fn prefixes_match_assembly_names() {
        assert_eq!(prefix(4, &[]), "o4/");
        assert_eq!(prefix(4, &[1, 22]), "o4/s1.22/");
        assert_eq!(prefix_of("o4/Box1:E1"), Some("o4/"));
        assert_eq!(prefix_of("o4/s1.22/Box1:Top"), Some("o4/s1.22/"));
        assert_eq!(prefix_of("o12/sketch1:E"), Some("o12/"));
        assert_eq!(prefix_of("Box1:Top"), None);
        let ls = [leaf(true), leaf(false)];
        assert_eq!(
            uncut(&[], StandardSection::ObeyBrowser, &ls),
            vec!["o3/s1.2/"]
        );
    }
}
