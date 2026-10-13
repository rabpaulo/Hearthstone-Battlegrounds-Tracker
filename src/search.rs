//! Fuzzy matching of card names against what the player typed.
//!
//! Tolerates what people actually type into a search box: partial words
//! ("bran" → Brann), skipped letters ("mtide" → Murloc Tidehunter), words
//! in any order ("riven tit" → Titus Rivendare) and small typos ("brnan").

/// Lower-case letters and digits only, words separated by single spaces.
/// Apostrophes vanish so "brann's" and "branns" are the same.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(fold(c));
        } else if c != '\'' && c != '’' {
            space = true;
        }
    }
    out
}

/// Strips the accents found in card names.
fn fold(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ñ' => 'n',
        'ç' => 'c',
        _ => c,
    }
}

/// A normalized query, ready to score many names.
pub struct Query {
    text: String,
    words: Vec<String>,
    /// The query without spaces, for subsequence matching.
    letters: Vec<char>,
}

impl Query {
    pub fn new(raw: &str) -> Query {
        let text = normalize(raw);
        let words = text.split(' ').filter(|w| !w.is_empty()).map(str::to_string).collect();
        let letters = text.chars().filter(|c| *c != ' ').collect();
        Query { text, words, letters }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// How well `name` matches; `None` when it does not. Higher is better.
    pub fn score(&self, name: &str) -> Option<i32> {
        if self.is_empty() {
            return Some(0);
        }
        let name = normalize(name);
        let len_penalty = name.len() as i32 / 4;
        if name == self.text {
            return Some(1000);
        }
        if name.starts_with(&self.text) {
            return Some(900 - len_penalty);
        }
        if let Some(at) = name.find(&self.text) {
            let word_start = name.as_bytes()[at - 1] == b' ';
            return Some(if word_start { 800 } else { 700 } - len_penalty - at as i32 / 4);
        }
        let words = self.word_score(&name);
        let subseq = subsequence_score(&self.letters, &name);
        words.max(subseq).map(|s| s - len_penalty)
    }

    /// Every query word matches some word of the name, in any order: as a
    /// prefix, an abbreviation, or within a typo or two.
    fn word_score(&self, name: &str) -> Option<i32> {
        let name_words: Vec<&str> = name.split(' ').collect();
        let mut cost = 0;
        for q in &self.words {
            cost += name_words.iter().filter_map(|w| word_cost(q, w)).min()?;
        }
        Some(600 - cost)
    }
}

/// How far query word `q` is from name word `w`, taking `q` as possibly
/// unfinished: 0 for a prefix, a little for an abbreviation ("brn" for
/// "brann"), more per typo; `None` past the typo allowance.
fn word_cost(q: &str, w: &str) -> Option<i32> {
    if w.starts_with(q) {
        return Some(0);
    }
    let q: Vec<char> = q.chars().collect();
    let w: Vec<char> = w.chars().collect();
    if q.len() >= 2 && q[0] == w[0] && is_subsequence(&q, &w) {
        return Some(25);
    }
    let allowed = match q.len() {
        0..=2 => 0,
        3..=6 => 1,
        _ => 2,
    };
    if allowed == 0 {
        return None;
    }
    // Whole word, or the prefix the player has typed so far (one letter of
    // slack either way for a dropped or doubled letter).
    let whole = osa_distance(&q, &w);
    let prefix = (q.len().saturating_sub(1)..=q.len() + 1)
        .filter(|&n| n > 0 && n < w.len())
        .map(|n| osa_distance(&q, &w[..n]))
        .min()
        .unwrap_or(usize::MAX);
    let d = whole.min(prefix);
    (d <= allowed).then_some(d as i32 * 60)
}

fn is_subsequence(q: &[char], w: &[char]) -> bool {
    let mut it = w.iter();
    q.iter().all(|c| it.any(|x| x == c))
}

/// Edit distance counting a swap of neighbours as one edit.
fn osa_distance(a: &[char], b: &[char]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[n][m]
}

/// The query's letters appear in order in the name, in runs that each
/// start a word ("mtide" → Murloc Tidehunter). Longer runs score higher.
fn subsequence_score(letters: &[char], name: &str) -> Option<i32> {
    if letters.len() < 2 {
        return None;
    }
    let chars: Vec<char> = name.chars().collect();
    let word_start = |i: usize| i == 0 || chars[i - 1] == ' ';
    let mut score = 0;
    let mut next = 0;
    let mut prev: Option<usize> = None;
    for &l in letters {
        let at = match prev {
            Some(p) if chars.get(p + 1) == Some(&l) => {
                score += 8;
                p + 1
            }
            _ => {
                score += 2;
                (next..chars.len()).find(|&i| chars[i] == l && word_start(i))?
            }
        };
        prev = Some(at);
        next = at + 1;
    }
    Some(300 + score.min(250))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rank<'a>(q: &str, names: &[&'a str]) -> Vec<&'a str> {
        let q = Query::new(q);
        let mut hits: Vec<(i32, &str)> = names.iter().filter_map(|n| Some((q.score(n)?, *n))).collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0));
        hits.into_iter().map(|(_, n)| n).collect()
    }

    const NAMES: &[&str] = &[
        "Brann Bronzebeard",
        "Baron Rivendare",
        "Mama Bear",
        "Titus Rivendare",
        "Lightfang Enforcer",
        "Bronze Warden",
        "Kalecgos, Arcane Aspect",
        "Murloc Tidehunter",
        "Tide Oracle Morgl",
        "Rylak Metalhead",
    ];

    #[test]
    fn normalizes_case_punctuation_and_accents() {
        assert_eq!(normalize("  Kalecgos, Arcane  Aspect "), "kalecgos arcane aspect");
        assert_eq!(normalize("Brann's Épée"), "branns epee");
    }

    #[test]
    fn prefix_beats_substring_beats_fuzzy() {
        assert_eq!(rank("bron", NAMES)[..2], ["Bronze Warden", "Brann Bronzebeard"]);
        assert_eq!(rank("rivendare", NAMES)[..2], ["Baron Rivendare", "Titus Rivendare"]);
    }

    #[test]
    fn words_in_any_order_and_unfinished() {
        assert_eq!(rank("riven tit", NAMES)[0], "Titus Rivendare");
        assert_eq!(rank("aspect kal", NAMES)[0], "Kalecgos, Arcane Aspect");
    }

    #[test]
    fn tolerates_typos() {
        assert_eq!(rank("brnan", NAMES)[0], "Brann Bronzebeard");
        assert_eq!(rank("rivnedare", NAMES)[..2], ["Baron Rivendare", "Titus Rivendare"]);
        assert_eq!(rank("lightfnag", NAMES)[0], "Lightfang Enforcer");
    }

    #[test]
    fn initials_and_skipped_letters() {
        assert_eq!(rank("mtide", NAMES)[0], "Murloc Tidehunter");
        assert_eq!(rank("rmetal", NAMES)[0], "Rylak Metalhead");
    }

    #[test]
    fn abbreviations_beat_typos() {
        assert_eq!(rank("lghtfng", NAMES)[0], "Lightfang Enforcer");
        let q = Query::new("brnn");
        assert!(q.score("Brann Bronzebeard") > q.score("Bronze Warden"));
    }

    #[test]
    fn rejects_unrelated_names() {
        assert!(rank("zzz", NAMES).is_empty());
        assert!(rank("tirion", NAMES).is_empty(), "letters scattered inside words");
        assert!(!rank("brann", NAMES).contains(&"Mama Bear"));
    }
}
