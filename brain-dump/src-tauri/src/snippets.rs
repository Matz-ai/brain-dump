use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snippet {
    pub trigger: String,
    pub text: String,
}

/// Replaces every spoken trigger with its snippet text. Matching ignores case, accents and the
/// punctuation Whisper adds, and the sentence-ending punctuation right after a trigger is dropped
/// so the snippet is pasted exactly as written. Runs after cleanup so the snippet text is untouched.
pub fn expand(text: &str, snippets: &[Snippet]) -> String {
    let mut triggers: Vec<(Vec<String>, &str)> = snippets
        .iter()
        .map(|s| (words(&s.trigger).into_iter().map(|(_, _, w)| w).collect::<Vec<_>>(), s.text.as_str()))
        .filter(|(t, _)| !t.is_empty())
        .collect();
    if triggers.is_empty() {
        return text.to_string();
    }
    // Longest first, so « ma signature pro » wins over « ma signature ».
    triggers.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    let tokens = words(text);
    let mut out = String::new();
    let mut copied = 0;
    let mut i = 0;
    while i < tokens.len() {
        let hit = triggers.iter().find(|(t, _)| {
            tokens.len() - i >= t.len() && tokens[i..i + t.len()].iter().zip(t).all(|(tok, w)| tok.2 == *w)
        });
        match hit {
            Some((t, replacement)) => {
                out.push_str(&text[copied..tokens[i].0]);
                out.push_str(replacement);
                let end = tokens[i + t.len() - 1].1;
                let rest = text[end..].trim_start();
                let after = rest.trim_start_matches(['.', '!', '?', '…']);
                copied = if after.len() < rest.len() { text.len() - after.len() } else { end };
                i += t.len();
            }
            None => i += 1,
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// Words with their byte span, normalized (lowercase, no accents).
fn words(text: &str) -> Vec<(usize, usize, String)> {
    let mut result = Vec::new();
    let mut start = None;
    for (idx, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(idx),
            (false, Some(s)) => {
                result.push((s, idx, normalize(&text[s..idx])));
                start = None;
            }
            _ => {}
        }
    }
    result
}

fn normalize(word: &str) -> String {
    let mut out = String::new();
    for c in word.to_lowercase().chars() {
        match c {
            'à' | 'â' | 'ä' | 'á' => out.push('a'),
            'é' | 'è' | 'ê' | 'ë' => out.push('e'),
            'î' | 'ï' | 'í' => out.push('i'),
            'ô' | 'ö' | 'ó' => out.push('o'),
            'ù' | 'û' | 'ü' | 'ú' => out.push('u'),
            'ç' => out.push('c'),
            'ÿ' => out.push('y'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snip(trigger: &str, text: &str) -> Snippet {
        Snippet { trigger: trigger.to_string(), text: text.to_string() }
    }

    const SIGNATURE: &str = "Mathieu
Brain Dump
06 00 00 00 00";

    #[test]
    fn test_whole_dictation_with_whisper_punctuation() {
        let s = [snip("ma signature", SIGNATURE)];
        assert_eq!(expand("Ma signature.", &s), SIGNATURE);
        assert_eq!(expand("Ma signature !", &s), SIGNATURE);
    }

    #[test]
    fn test_ignores_case_and_accents() {
        let s = [snip("mon adresse e-mail", "moi@exemple.fr")];
        assert_eq!(expand("Mon Adresse E mail.", &s), "moi@exemple.fr");
        let s = [snip("Numéro Pro", "01 23 45 67 89")];
        assert_eq!(expand("numero pro.", &s), "01 23 45 67 89");
    }

    #[test]
    fn test_mid_sentence() {
        let s = [snip("ma signature", SIGNATURE)];
        assert_eq!(expand("Merci, ma signature.", &s), format!("Merci, {}", SIGNATURE));
        assert_eq!(
            expand("Voici le lien calendly. À bientôt.", &[snip("lien calendly", "https://calendly.com/moi")]),
            "Voici le https://calendly.com/moi À bientôt."
        );
    }

    #[test]
    fn test_several_snippets() {
        let s = [
            snip("ma signature", "SIG"),
            snip("ma signature pro", "SIG PRO"),
            snip("mon adresse", "1 rue de Paris"),
        ];
        assert_eq!(
            expand("Mon adresse, ma signature pro.", &s),
            "1 rue de Paris, SIG PRO"
        );
        assert_eq!(expand("Ma signature et ma signature.", &s), "SIG et SIG");
    }

    #[test]
    fn test_no_match() {
        let s = [snip("ma signature", "SIG"), snip("", "vide")];
        assert_eq!(expand("Ma signa ture est illisible.", &s), "Ma signa ture est illisible.");
        assert_eq!(expand("Ma signaturette.", &s), "Ma signaturette.");
        assert_eq!(expand("Bonjour.", &[]), "Bonjour.");
    }
}
