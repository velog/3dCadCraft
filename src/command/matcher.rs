//! Matching typed text against commands: the alias input next to the cursor
//! and the command search (Alt+S) share it (`docs/spec/0.15-context-menus-shortcuts.md`
//! §5.1 rule 4 and §5.3).
//!
//! GUI-free: a command is just its texts ([`Fields`]), folded once into a
//! [`Folded`] (lower case, Cyrillic transliterated to Latin, so `изт`,
//! `IZT` and `izt` all find "Изтегляне"). [`rank`] puts a match into a
//! [`Tier`] and gives a score inside it:
//!
//! 1. [`Tier::Exact`] — the text *is* an alias or a whole name;
//! 2. [`Tier::Recent`] — any match of a recently used command;
//! 3. [`Tier::AliasPrefix`] — an alias starts with the text;
//! 4. [`Tier::WordPrefix`] — a word of a name starts with the text
//!    ("ext" → Extrude, Extend; "two p" → Two Point Rectangle);
//! 5. [`Tier::Fuzzy`] — a substring or the letters in order (word starts and
//!    runs score higher), or a word of the tooltip.
//!
//! `*` (any run) and `?` (one character) work as in Inventor's search: a
//! pattern is anchored at a word start of a name (and open at its end), or
//! must cover a whole alias.
//!
//! Which commands are valid in the current environment, or not implemented
//! yet, is the caller's business (`gui::commands` sorts those last).

/// How a command matched; earlier tiers sort first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// The text is one of the aliases, or a whole name.
    Exact,
    /// A recently used command that matches in any way.
    Recent,
    /// An alias starts with the text.
    AliasPrefix,
    /// A word of a name starts with the text.
    WordPrefix,
    /// Substring, letters in order, or the tooltip.
    Fuzzy,
}

/// The texts of one command, as given.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fields<'a> {
    /// Short names typed at the keyboard (`E`, `SW`, `]`).
    pub aliases: &'a [&'a str],
    /// English name, Inventor's name, Bulgarian name, ribbon label…
    pub names: &'a [&'a str],
    /// Tooltip text.
    pub tip: &'a str,
}

/// [`Fields`] folded for matching (built once per command).
#[derive(Clone, Debug, Default)]
pub struct Folded {
    aliases: Vec<String>,
    names: Vec<String>,
    tip: String,
}

impl Folded {
    /// Folds every text of `f`.
    pub fn new(f: &Fields<'_>) -> Self {
        Self {
            aliases: f.aliases.iter().map(|a| fold(a)).collect(),
            names: f.names.iter().map(|n| fold(n)).collect(),
            tip: fold(f.tip),
        }
    }
}

/// Lower case, Bulgarian Cyrillic transliterated to Latin (Streamlined
/// System), whitespace runs collapsed to one space, ends trimmed.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.trim().chars().flat_map(char::to_lowercase) {
        if c.is_whitespace() {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            continue;
        }
        match translit(c) {
            Some(t) => out.push_str(t),
            None => out.push(c),
        }
    }
    out
}

/// Latin for a lower-case Cyrillic letter.
fn translit(c: char) -> Option<&'static str> {
    Some(match c {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "g",
        'д' => "d",
        'е' | 'э' | 'ё' => "e",
        'ж' => "zh",
        'з' => "z",
        'и' | 'ы' => "i",
        'й' => "y",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "h",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "sht",
        'ъ' => "a",
        'ь' => "y",
        'ю' => "yu",
        'я' => "ya",
        _ => return None,
    })
}

/// A typed text, folded.
#[derive(Clone, Debug, Default)]
pub struct Query {
    text: String,
    /// `text` without spaces (for letters-in-order matching).
    compact: Vec<char>,
    glob: Option<Vec<char>>,
}

impl Query {
    /// Folds `s`; `*` / `?` make it a pattern.
    pub fn new(s: &str) -> Self {
        let text = fold(s);
        let glob = (text.contains('*') || text.contains('?')).then(|| text.chars().collect());
        let compact = text.chars().filter(|c| *c != ' ').collect();
        Self {
            text,
            compact,
            glob,
        }
    }

    /// Nothing typed.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The folded text.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Byte offsets where a word starts in a folded text.
fn word_starts(s: &str) -> impl Iterator<Item = usize> + '_ {
    let mut prev_alnum = false;
    s.char_indices().filter_map(move |(i, c)| {
        let start = c.is_alphanumeric() && !prev_alnum;
        prev_alnum = c.is_alphanumeric();
        start.then_some(i)
    })
}

/// `pattern` (with `*` and `?`) against the start of `text`; with `whole`
/// it must cover all of `text`, otherwise only its start.
fn glob(pattern: &[char], text: &[char], whole: bool) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    loop {
        if p == pattern.len() && (!whole || t == text.len()) {
            return true;
        }
        if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, t));
            p += 1;
            continue;
        }
        if t < text.len() && p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
            continue;
        }
        match star {
            Some((sp, st)) if st < text.len() => {
                star = Some((sp, st + 1));
                p = sp + 1;
                t = st + 1;
            }
            _ => return false,
        }
    }
}

/// Letters of `q` in order in `s`: a score (word starts and runs count
/// more, gaps less), or `None`.
fn subsequence(q: &[char], s: &str) -> Option<i32> {
    if q.is_empty() {
        return None;
    }
    let mut score = 0;
    let mut k = 0;
    let mut prev: Option<char> = None;
    let mut run = false;
    for c in s.chars() {
        if k < q.len() && c == q[k] {
            let at_word = prev.is_none_or(|p| !p.is_alphanumeric());
            score += if at_word { 10 } else { 1 } + if run { 5 } else { 0 };
            if k == 0 && prev.is_none() {
                score += 10;
            }
            k += 1;
            run = true;
        } else {
            run = false;
            if k > 0 && k < q.len() {
                score -= 1;
            }
        }
        prev = Some(c);
    }
    (k == q.len()).then_some(score)
}

/// How `q` matches command `c`: the tier and a score inside it (higher is
/// better), or `None`. `recent` is the command's place in the recent list
/// (0 = newest), if it is there. An empty query matches only recent
/// commands.
pub fn rank(q: &Query, c: &Folded, recent: Option<usize>) -> Option<(Tier, i32)> {
    let promote = |m: Option<(Tier, i32)>| match (m, recent) {
        (Some((t, _)), Some(r)) if t > Tier::Exact => Some((Tier::Recent, -(r as i32))),
        _ => m,
    };
    if q.is_empty() {
        return recent.map(|r| (Tier::Recent, -(r as i32)));
    }
    if let Some(p) = &q.glob {
        return promote(rank_glob(p, c));
    }
    let t = q.text.as_str();
    if c.aliases.iter().any(|a| a == t) || c.names.iter().any(|n| n == t) {
        return Some((Tier::Exact, 0));
    }
    let mut best: Option<(Tier, i32)> = None;
    let mut offer = |m: (Tier, i32)| {
        if best.is_none_or(|b| (m.0, -m.1) < (b.0, -b.1)) {
            best = Some(m);
        }
    };
    for a in &c.aliases {
        if a.starts_with(t) {
            // Shorter aliases first (S before SW).
            offer((Tier::AliasPrefix, -(a.len() as i32)));
        }
    }
    for (k, n) in c.names.iter().enumerate() {
        // Earlier names (English) a little before later ones.
        let pref = -(k as i32);
        if n.starts_with(t) {
            offer((Tier::WordPrefix, 200 + pref - n.len() as i32 / 8));
        } else if word_starts(n).any(|i| n[i..].starts_with(t)) {
            offer((Tier::WordPrefix, 100 + pref - n.len() as i32 / 8));
        } else if let Some(i) = n.find(t) {
            offer((Tier::Fuzzy, 1000 - i as i32));
        } else if q.compact.len() >= 2
            && let Some(s) = subsequence(&q.compact, n)
        {
            offer((Tier::Fuzzy, 500 + s));
        }
    }
    if best.is_none() && t.len() >= 2 && word_starts(&c.tip).any(|i| c.tip[i..].starts_with(t)) {
        best = Some((Tier::Fuzzy, 100));
    }
    promote(best)
}

fn rank_glob(p: &[char], c: &Folded) -> Option<(Tier, i32)> {
    let chars = |s: &str| -> Vec<char> { s.chars().collect() };
    if c.aliases.iter().any(|a| glob(p, &chars(a), true)) {
        return Some((Tier::AliasPrefix, 0));
    }
    let at_word = |s: &str| word_starts(s).any(|i| glob(p, &chars(&s[i..]), false));
    if let Some(k) = c.names.iter().position(|n| at_word(n)) {
        return Some((Tier::WordPrefix, -(k as i32)));
    }
    at_word(&c.tip).then_some((Tier::Fuzzy, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(aliases: &[&str], names: &[&str], tip: &str) -> Folded {
        Folded::new(&Fields {
            aliases,
            names,
            tip,
        })
    }

    fn tier(q: &str, c: &Folded) -> Option<Tier> {
        rank(&Query::new(q), c, None).map(|r| r.0)
    }

    #[test]
    fn folding_lowercases_and_transliterates() {
        assert_eq!(fold("  Изтегляне  на ТЯЛО "), "izteglyane na tyalo");
        assert_eq!(fold("Щит Ъгъл Юг"), "shtit agal yug");
        assert_eq!(fold("Work Plane"), "work plane");
    }

    #[test]
    fn tiers() {
        let extrude = cmd(&["E"], &["Extrude", "Изтегляне"], "Extrude closed regions");
        let extend = cmd(
            &["EX"],
            &["Extend", "Удължаване"],
            "Extend to the nearest curve",
        );
        let rect = cmd(&["R", "REC"], &["Two Point Rectangle"], "");
        assert_eq!(tier("e", &extrude), Some(Tier::Exact));
        assert_eq!(tier("e", &extend), Some(Tier::AliasPrefix));
        assert_eq!(tier("ext", &extrude), Some(Tier::WordPrefix));
        assert_eq!(tier("EXTRUDE", &extrude), Some(Tier::Exact));
        assert_eq!(tier("point", &rect), Some(Tier::WordPrefix));
        assert_eq!(tier("two p", &rect), Some(Tier::WordPrefix));
        assert_eq!(tier("rectngl", &rect), Some(Tier::Fuzzy));
        assert_eq!(tier("oint", &rect), Some(Tier::Fuzzy));
        assert_eq!(tier("closed", &extrude), Some(Tier::Fuzzy), "tooltip");
        assert_eq!(tier("изт", &extrude), Some(Tier::WordPrefix));
        assert_eq!(
            tier("izt", &extrude),
            Some(Tier::WordPrefix),
            "Latin for Cyrillic"
        );
        assert_eq!(tier("qq", &extrude), None);
        assert_eq!(tier("", &extrude), None);
        // A recent command beats prefixes, never an exact alias.
        let q = Query::new("exte");
        assert_eq!(rank(&q, &extend, Some(3)).map(|r| r.0), Some(Tier::Recent));
        assert_eq!(
            rank(&Query::new("e"), &extrude, Some(0)).map(|r| r.0),
            Some(Tier::Exact)
        );
        assert_eq!(
            rank(&Query::new(""), &extrude, Some(2)),
            Some((Tier::Recent, -2))
        );
    }

    #[test]
    fn wildcards() {
        let pat = cmd(&["RP"], &["Rectangular Pattern"], "Copy features in rows");
        let line = cmd(&["L"], &["Line"], "");
        assert_eq!(tier("rect*", &pat), Some(Tier::WordPrefix));
        assert_eq!(tier("*pattern", &pat), Some(Tier::WordPrefix));
        assert_eq!(tier("pat?ern", &pat), Some(Tier::WordPrefix));
        assert_eq!(tier("r?", &pat), Some(Tier::AliasPrefix));
        assert_eq!(tier("?ine", &line), Some(Tier::WordPrefix));
        assert_eq!(tier("x*z", &line), None);
        assert!(glob(&['a', '*', 'c'], &['a', 'b', 'b', 'c'], true));
        assert!(!glob(&['a', '?'], &['a'], true));
    }
}
