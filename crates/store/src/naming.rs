//! Identifier naming — ports Sequelize's `Utils.underscoredIf(Utils.pluralize(x), true)`
//! used by `sequelizeUtil.ts#modelToTableName` and column naming (`underscored`).
//!
//! Getting these byte-identical matters: they determine table and column names, so
//! any divergence produces a different schema than the TS node. Rules are ported
//! from the `inflection` library that Sequelize uses.

use regex::Regex;
use std::sync::OnceLock;

/// `inflection.underscore` — camelCase/PascalCase → snake_case, acronym-aware.
/// e.g. `blockHeight` → `block_height`, `HTTPServer` → `http_server`.
pub fn underscored(input: &str) -> String {
    static ACRONYM: OnceLock<Regex> = OnceLock::new();
    static WORD: OnceLock<Regex> = OnceLock::new();
    let acronym = ACRONYM.get_or_init(|| Regex::new(r"([A-Z\d]+)([A-Z][a-z])").unwrap());
    let word = WORD.get_or_init(|| Regex::new(r"([a-z\d])([A-Z])").unwrap());

    let step1 = acronym.replace_all(input, "${1}_${2}");
    let step2 = word.replace_all(&step1, "${1}_${2}");
    step2.replace('-', "_").to_lowercase()
}

/// `inflection.pluralize` — English pluralization matching the ordered rule set
/// Sequelize relies on. Covers uncountables, irregulars, then regex rules.
pub fn pluralize(word: &str) -> String {
    if word.is_empty() {
        return String::new();
    }
    let lower = word.to_lowercase();

    // Uncountable nouns are returned unchanged.
    const UNCOUNTABLE: &[&str] = &[
        "equipment",
        "information",
        "rice",
        "money",
        "species",
        "series",
        "fish",
        "sheep",
        "jeans",
        "moose",
        "deer",
        "news",
    ];
    if UNCOUNTABLE.contains(&lower.as_str()) {
        return word.to_string();
    }

    // Irregular plurals (preserving the leading character's case as inflection does).
    const IRREGULAR: &[(&str, &str)] = &[
        ("person", "people"),
        ("man", "men"),
        ("child", "children"),
        ("sex", "sexes"),
        ("move", "moves"),
        ("cow", "kine"),
        ("zombie", "zombies"),
    ];
    for (sing, plur) in IRREGULAR {
        if lower == *sing {
            // Re-apply the original first-letter case.
            let mut result = plur.to_string();
            if word
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
            {
                result = capitalize_first(&result);
            }
            return result;
        }
    }

    // Ordered regex rules — first match wins. (regex, replacement)
    static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    let rules = RULES.get_or_init(|| {
        let raw: &[(&str, &str)] = &[
            (r"(?i)(quiz)$", "${1}zes"),
            (r"(?i)^(ox)$", "${1}en"),
            (r"(?i)([ml])ouse$", "${1}ice"),
            (r"(?i)(matr|vert|ind)(ix|ex)$", "${1}ices"),
            (r"(?i)(x|ch|ss|sh)$", "${1}es"),
            (r"(?i)([^aeiouy]|qu)y$", "${1}ies"),
            (r"(?i)(hive)$", "${1}s"),
            (r"(?i)(?:([^f])fe|([lr])f)$", "${1}${2}ves"),
            (r"(?i)sis$", "ses"),
            (r"(?i)([ti])um$", "${1}a"),
            (r"(?i)(buffal|tomat)o$", "${1}oes"),
            (r"(?i)(bu)s$", "${1}ses"),
            (r"(?i)(alias|status)$", "${1}es"),
            (r"(?i)(octop|vir)us$", "${1}i"),
            (r"(?i)(ax|test)is$", "${1}es"),
            (r"(?i)s$", "s"),
            (r"(?i)$", "s"),
        ];
        raw.iter()
            .map(|(re, rep)| (Regex::new(re).unwrap(), *rep))
            .collect()
    });

    for (re, rep) in rules {
        if re.is_match(word) {
            return re.replace(word, *rep).into_owned();
        }
    }
    word.to_string()
}

/// `modelToTableName` — `underscored(pluralize(name))`.
pub fn model_to_table_name(model_name: &str) -> String {
    underscored(&pluralize(model_name))
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underscore_cases() {
        assert_eq!(underscored("blockHeight"), "block_height");
        assert_eq!(underscored("Transfers"), "transfers");
        assert_eq!(underscored("HTTPServer"), "http_server");
        assert_eq!(underscored("id"), "id");
        assert_eq!(underscored("myLongFieldName"), "my_long_field_name");
    }

    #[test]
    fn pluralize_regular() {
        assert_eq!(pluralize("Transfer"), "Transfers");
        assert_eq!(pluralize("account"), "accounts");
    }

    #[test]
    fn pluralize_irregular_and_rules() {
        assert_eq!(pluralize("entity"), "entities");
        assert_eq!(pluralize("category"), "categories");
        assert_eq!(pluralize("box"), "boxes");
        assert_eq!(pluralize("person"), "people");
        assert_eq!(pluralize("status"), "statuses");
    }

    #[test]
    fn table_name_matches_ts() {
        // Ground truth from the TS fixture: Transfer -> transfers.
        assert_eq!(model_to_table_name("Transfer"), "transfers");
        assert_eq!(model_to_table_name("MyEntity"), "my_entities");
    }
}
