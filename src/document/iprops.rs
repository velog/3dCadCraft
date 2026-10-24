//! iProperties (`docs/spec/0.16-iproperties.md`): the descriptive properties
//! of every document — Summary, Project and Status fields by key, the
//! Design State, and custom properties of the user's own (Text, Date,
//! Number, Yes or No). Stored on [`super::Document`], skipped when empty.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The tab a standard property is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// Summary.
    Summary,
    /// Project.
    Project,
    /// Status.
    Status,
}

/// Every standard property: (key, tab, label). Keys are stored in files:
/// never rename one.
pub const STANDARD: &[(&str, Tab, &str)] = &[
    ("title", Tab::Summary, "Title"),
    ("subject", Tab::Summary, "Subject"),
    ("author", Tab::Summary, "Author"),
    ("manager", Tab::Summary, "Manager"),
    ("company", Tab::Summary, "Company"),
    ("category", Tab::Summary, "Category"),
    ("keywords", Tab::Summary, "Keywords"),
    ("comments", Tab::Summary, "Comments"),
    ("part_number", Tab::Project, "Part Number"),
    ("stock_number", Tab::Project, "Stock Number"),
    ("description", Tab::Project, "Description"),
    ("revision", Tab::Project, "Revision Number"),
    ("project", Tab::Project, "Project"),
    ("designer", Tab::Project, "Designer"),
    ("engineer", Tab::Project, "Engineer"),
    ("authority", Tab::Project, "Authority"),
    ("cost_center", Tab::Project, "Cost Center"),
    ("estimated_cost", Tab::Project, "Estimated Cost"),
    ("creation_date", Tab::Project, "Creation Date"),
    ("vendor", Tab::Project, "Vendor"),
    ("web_link", Tab::Project, "WEB Link"),
    ("status", Tab::Status, "Status"),
    ("checked_by", Tab::Status, "Checked By"),
    ("checked_date", Tab::Status, "Checked Date"),
    ("eng_approved_by", Tab::Status, "Eng. Approved By"),
    ("eng_approved_date", Tab::Status, "Eng. Approved Date"),
    ("mfg_approved_by", Tab::Status, "Mfg. Approved By"),
    ("mfg_approved_date", Tab::Status, "Mfg. Approved Date"),
];

/// Where a design is in its life.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DesignState {
    /// Being worked on (the default).
    #[default]
    WorkInProgress,
    /// Waiting for approval.
    Pending,
    /// Released.
    Released,
}

impl DesignState {
    /// Every state, in dialog order.
    pub const ALL: [DesignState; 3] = [
        DesignState::WorkInProgress,
        DesignState::Pending,
        DesignState::Released,
    ];

    /// Label as Inventor shows it.
    pub fn label(self) -> &'static str {
        match self {
            DesignState::WorkInProgress => "Work In Progress",
            DesignState::Pending => "Pending",
            DesignState::Released => "Released",
        }
    }
}

/// A custom property's value and type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum PropValue {
    /// Text.
    Text(String),
    /// A date, `YYYY-MM-DD`.
    Date(String),
    /// A number.
    Number(f64),
    /// Yes or No.
    YesNo(bool),
}

impl PropValue {
    /// The type's name (Custom tab).
    pub fn type_label(&self) -> &'static str {
        match self {
            PropValue::Text(_) => "Text",
            PropValue::Date(_) => "Date",
            PropValue::Number(_) => "Number",
            PropValue::YesNo(_) => "Yes or No",
        }
    }

    /// The value as text.
    pub fn text(&self) -> String {
        match self {
            PropValue::Text(s) | PropValue::Date(s) => s.clone(),
            PropValue::Number(v) => format!("{v}"),
            PropValue::YesNo(b) => if *b { "Yes" } else { "No" }.into(),
        }
    }
}

/// A property of the user's own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomProperty {
    /// Its name (unique, not a standard name).
    pub name: String,
    /// Its value and type.
    pub value: PropValue,
}

/// The iProperties of a document.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IProperties {
    /// Standard properties by key ([`STANDARD`]); empty ones are absent.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub standard: BTreeMap<String, String>,
    /// Status › Design State.
    #[serde(default, skip_serializing_if = "is_default_state")]
    pub design_state: DesignState,
    /// Custom properties, in the order added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom: Vec<CustomProperty>,
}

fn is_default_state(s: &DesignState) -> bool {
    *s == DesignState::WorkInProgress
}

impl IProperties {
    /// Nothing set (skipped in files).
    pub fn is_empty(&self) -> bool {
        self.standard.is_empty() && is_default_state(&self.design_state) && self.custom.is_empty()
    }

    /// The properties of a new document: Author and Designer = the login
    /// name, Creation Date = today.
    pub fn new_document() -> Self {
        let mut p = Self::default();
        if let Some(user) = ["USERNAME", "USER"]
            .iter()
            .find_map(|k| std::env::var(k).ok())
            .filter(|u| !u.is_empty())
        {
            p.set("author", &user);
            p.set("designer", &user);
        }
        p.set("creation_date", &today());
        p
    }

    /// The stored value of standard property `key` ("" when not set).
    pub fn get(&self, key: &str) -> &str {
        self.standard.get(key).map_or("", String::as_str)
    }

    /// Sets standard property `key` (an empty value removes it).
    pub fn set(&mut self, key: &str, value: &str) {
        if value.is_empty() {
            self.standard.remove(key);
        } else {
            self.standard.insert(key.to_owned(), value.to_owned());
        }
    }

    /// The value with its default: an empty Part Number is the file's
    /// (or document's) name.
    pub fn effective(&self, key: &str, file_name: &str) -> String {
        match self.get(key) {
            "" if key == "part_number" => file_name.to_owned(),
            v => v.to_owned(),
        }
    }

    /// Adds custom property `name`.
    pub fn add_custom(&mut self, name: &str, value: PropValue) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("give the property a name".into());
        }
        let lower = name.to_lowercase();
        if STANDARD
            .iter()
            .any(|(k, _, l)| l.to_lowercase() == lower || *k == lower)
            || lower == "design state"
        {
            return Err(format!("{name} is a standard property"));
        }
        if self.custom.iter().any(|c| c.name.to_lowercase() == lower) {
            return Err(format!("there is already a property named {name}"));
        }
        self.custom.push(CustomProperty {
            name: name.to_owned(),
            value,
        });
        Ok(())
    }
}

/// Today as `YYYY-MM-DD` (UTC).
pub fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    date_of_days((secs / 86_400) as i64)
}

/// The civil date `YYYY-MM-DD` of a day count from 1970-01-01.
pub fn date_of_days_pub(z: i64) -> String {
    date_of_days(z)
}

/// The civil date of a day count from 1970-01-01 (H. Hinnant's algorithm).
fn date_of_days(z: i64) -> String {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Whether `s` is a date `YYYY-MM-DD`.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && s.split('-').all(|p| p.chars().all(|c| c.is_ascii_digit()))
        && s[5..7].parse::<u32>().is_ok_and(|m| (1..=12).contains(&m))
        && s[8..10].parse::<u32>().is_ok_and(|d| (1..=31).contains(&d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_custom_names_and_dates() {
        let mut p = IProperties::new_document();
        assert!(
            is_date(p.get("creation_date")),
            "{}",
            p.get("creation_date")
        );
        assert_eq!(p.effective("part_number", "bracket"), "bracket");
        p.set("part_number", "BR-100");
        assert_eq!(p.effective("part_number", "bracket"), "BR-100");
        p.set("part_number", "");
        assert!(!p.standard.contains_key("part_number"));
        p.add_custom("Material Code", PropValue::Text("S235".into()))
            .unwrap();
        assert!(
            p.add_custom("material code", PropValue::Number(1.0))
                .is_err()
        );
        assert!(
            p.add_custom("Part Number", PropValue::Text("x".into()))
                .is_err()
        );
        assert!(p.add_custom("  ", PropValue::YesNo(true)).is_err());
        assert_eq!(date_of_days(0), "1970-01-01");
        assert_eq!(date_of_days(20_750), "2026-10-24");
        assert!(!is_date("2026-13-01") && !is_date("26-10-24"));
        // Round trip, and nothing stored for an empty set.
        let json = serde_json::to_string(&p).unwrap();
        let back: IProperties = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        assert!(IProperties::default().is_empty());
    }
}
