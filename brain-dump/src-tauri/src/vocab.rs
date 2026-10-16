use serde::{Deserialize, Serialize};

use crate::snippets::{normalize, words};

/// Taille max du vocabulaire envoyé à Whisper (prompt limité à ~224 tokens chez Groq).
/// Garder en phase avec WHISPER_VOCAB_MAX_CHARS dans src/main.ts.
pub const WHISPER_PROMPT_MAX_CHARS: usize = 550;

/// Paire apprise : ce que Whisper entend → ce que je voulais dire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Replacement {
    pub from: String,
    pub to: String,
}

pub type Suggestion = Replacement;

/// Prompt Whisper : les mots les plus récents (fin de liste) qui tiennent dans la limite.
pub fn whisper_prompt(vocabulary: &str) -> String {
    let mut kept = Vec::new();
    let mut len = 0;
    for word in vocabulary.split([',', '\n']).map(str::trim).filter(|w| !w.is_empty()).rev() {
        let cost = word.chars().count() + if kept.is_empty() { 0 } else { 2 };
        if len + cost > WHISPER_PROMPT_MAX_CHARS {
            break;
        }
        len += cost;
        kept.push(word);
    }
    kept.reverse();
    kept.join(", ")
}

/// Remplace chaque expression apprise, sans tenir compte de la casse, des accents ni de la ponctuation.
pub fn apply_replacements(text: &str, replacements: &[Replacement]) -> String {
    let mut pairs: Vec<(Vec<String>, &str)> = replacements
        .iter()
        .map(|r| (words(&r.from).into_iter().map(|(_, _, w)| w).collect::<Vec<_>>(), r.to.as_str()))
        .filter(|(from, _)| !from.is_empty())
        .collect();
    if pairs.is_empty() {
        return text.to_string();
    }
    pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let tokens = words(text);
    let mut out = String::new();
    let mut copied = 0;
    let mut i = 0;
    while i < tokens.len() {
        let hit = pairs.iter().find(|(from, _)| {
            tokens.len() - i >= from.len() && tokens[i..i + from.len()].iter().zip(from).all(|(t, w)| t.2 == *w)
        });
        match hit {
            Some((from, to)) => {
                out.push_str(&text[copied..tokens[i].0]);
                out.push_str(to);
                copied = tokens[i + from.len() - 1].1;
                i += from.len();
            }
            None => i += 1,
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// Mots retouchés à la main qui ressemblent à du vocabulaire (nom propre, jargon, mot mal découpé).
/// Ignore la ponctuation, la casse, les accents et les simples corrections d'accord.
pub fn suggestions(original: &str, edited: &str) -> Vec<Suggestion> {
    let old = tokenize(original);
    let new = tokenize(edited);
    let mut result: Vec<Suggestion> = Vec::new();
    for (o, n) in diff_hunks(&old, &new) {
        if o.is_empty() || n.is_empty() || o.len() > 6 || n.len() > 4 {
            continue;
        }
        let looks_like_term = n.iter().any(|t| t.is_term_like())
            || (o.len() != n.len() && is_resplit(o, n));
        if !looks_like_term {
            continue;
        }
        let s = Suggestion { from: join(o), to: join(n) };
        if !result.contains(&s) {
            result.push(s);
        }
    }
    result
}

struct Token {
    text: String,
    norm: String,
    sentence_start: bool,
}

impl Token {
    fn is_term_like(&self) -> bool {
        let mut chars = self.text.chars();
        let first_upper = chars.next().is_some_and(char::is_uppercase);
        (first_upper && !self.sentence_start)
            || chars.any(|c| c.is_uppercase())
            || self.text.chars().any(|c| c.is_ascii_digit() || !c.is_alphanumeric())
    }
}

/// « super base » → « supabase », « web hook » → « webhook » : même mot, mal découpé.
fn is_resplit(old: &[Token], new: &[Token]) -> bool {
    let a: String = old.iter().map(|t| t.norm.as_str()).collect();
    let b: String = new.iter().map(|t| t.norm.as_str()).collect();
    levenshtein(&a, &b) <= (a.chars().count().max(b.chars().count()) / 3).max(1)
}

fn join(tokens: &[Token]) -> String {
    tokens.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join(" ")
}

/// Mots séparés par espaces et apostrophes, ponctuation de bord retirée (« Next.js » reste entier).
fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut sentence_start = true;
    for chunk in text.split(|c: char| c.is_whitespace() || c == '\'' || c == '’') {
        let word = chunk.trim_matches(|c: char| !c.is_alphanumeric());
        if !word.is_empty() {
            tokens.push(Token { text: word.to_string(), norm: normalize(word), sentence_start });
            sentence_start = false;
        }
        let tail = chunk.rsplit(char::is_alphanumeric).next().unwrap_or("");
        if tail.contains(['.', '!', '?', '…']) {
            sentence_start = true;
        }
    }
    tokens
}

/// Blocs (ancien, nouveau) qui diffèrent, d'après la plus longue sous-suite commune.
fn diff_hunks<'a>(old: &'a [Token], new: &'a [Token]) -> Vec<(&'a [Token], &'a [Token])> {
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if old[i].norm == new[j].norm {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut hunks = Vec::new();
    let (mut i, mut j, mut hunk_start) = (0, 0, (0, 0));
    while i < n || j < m {
        if i < n && j < m && old[i].norm == new[j].norm {
            if hunk_start != (i, j) {
                hunks.push((&old[hunk_start.0..i], &new[hunk_start.1..j]));
            }
            i += 1;
            j += 1;
            hunk_start = (i, j);
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            j += 1;
        } else {
            i += 1;
        }
    }
    if hunk_start != (n, m) {
        hunks.push((&old[hunk_start.0..], &new[hunk_start.1..]));
    }
    hunks
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + (ca != *cb) as usize).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(from: &str, to: &str) -> Replacement {
        Replacement { from: from.to_string(), to: to.to_string() }
    }

    #[test]
    fn test_simple_replacement() {
        assert_eq!(
            suggestions("J'utilise super base pour l'app.", "J'utilise Supabase pour l'app."),
            vec![pair("super base", "Supabase")]
        );
        // « api » → « API » : casse seule, ignoré
        assert_eq!(
            suggestions("On appelle l'api de groc.", "On appelle l'API de Groq."),
            vec![pair("groc", "Groq")]
        );
    }

    #[test]
    fn test_multi_word_and_jargon() {
        assert_eq!(
            suggestions("Le projet next j s marche.", "Le projet Next.js marche."),
            vec![pair("next j s", "Next.js")]
        );
        // Mot mal découpé, en minuscules
        assert_eq!(
            suggestions("Ajoute un web hook.", "Ajoute un webhook."),
            vec![pair("web hook", "webhook")]
        );
        assert_eq!(
            suggestions("Envoie ça à paul demain", "Envoie ça à Pierre-Louis demain"),
            vec![pair("paul", "Pierre-Louis")]
        );
    }

    #[test]
    fn test_punctuation_case_and_agreement_ignored() {
        assert!(suggestions("bonjour comment ça va", "Bonjour, comment ça va ?").is_empty());
        assert!(suggestions("les pommes sont mangé", "Les pommes sont mangées.").is_empty());
        assert!(suggestions("il a ete", "il a été").is_empty());
        assert!(suggestions("le maison", "la maison").is_empty());
        // Majuscule de début de phrase : pas un nom propre
        assert!(suggestions("Fini. le chat dort.", "Fini. La chatte dort.").is_empty());
        // Retrait de mots seul : rien à apprendre
        assert!(suggestions("Euh je pense que oui", "Je pense que oui").is_empty());
    }

    #[test]
    fn test_unrelated_rewrite_not_suggested() {
        // Mots supprimés + remplacés sans ressemblance, tout en minuscules
        assert!(suggestions("euh le truc", "la chose").is_empty());
    }

    #[test]
    fn test_apply_replacements() {
        let reps = [pair("super base", "Supabase"), pair("groc", "Groq")];
        assert_eq!(apply_replacements("J'utilise Super Base et groc.", &reps), "J'utilise Supabase et Groq.");
        assert_eq!(apply_replacements("Super-base, super base !", &reps), "Supabase, Supabase !");
        // Pas de correspondance partielle de mot
        assert_eq!(apply_replacements("superbase grocery", &reps), "superbase grocery");
        assert_eq!(apply_replacements("rien", &[]), "rien");
    }

    #[test]
    fn test_whisper_prompt_keeps_most_recent() {
        assert_eq!(whisper_prompt("Supabase, Groq,\nTauri"), "Supabase, Groq, Tauri");
        let vocab: Vec<String> = (0..300).map(|i| format!("mot{:03}", i)).collect();
        let prompt = whisper_prompt(&vocab.join(", "));
        assert!(prompt.chars().count() <= WHISPER_PROMPT_MAX_CHARS);
        assert!(prompt.ends_with("mot299"));
        assert!(!prompt.contains("mot000"));
    }
}
