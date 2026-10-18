pub fn cleanup_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Normalize multiple spaces to single space
    let normalized: String = trimmed
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ");

    // Capitalize first letter of each sentence. A sentence ends only when the punctuation is
    // followed by a space: "0.4.0 de" or "google.com" are not sentence ends.
    let chars: Vec<char> = normalized.chars().collect();
    let mut result = String::new();
    let mut capitalize_next = true;

    for (i, &ch) in chars.iter().enumerate() {
        if capitalize_next && ch.is_alphabetic() {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
        if matches!(ch, '.' | '!' | '?') && chars.get(i + 1) == Some(&' ') {
            capitalize_next = true;
        }
    }

    // Ensure ending punctuation
    if let Some(last) = result.chars().last() {
        if !matches!(last, '.' | '!' | '?') {
            result.push('.');
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trim_whitespace() {
        assert_eq!(cleanup_text("  hello world  "), "Hello world.");
    }

    #[test]
    fn test_normalize_spaces() {
        assert_eq!(cleanup_text("hello    world"), "Hello world.");
    }

    #[test]
    fn test_capitalize_first_letter() {
        assert_eq!(cleanup_text("hello world"), "Hello world.");
    }

    #[test]
    fn test_capitalize_after_period() {
        assert_eq!(cleanup_text("hello. world"), "Hello. World.");
    }

    #[test]
    fn test_capitalize_after_question_mark() {
        assert_eq!(cleanup_text("hello? world"), "Hello? World.");
    }

    #[test]
    fn test_capitalize_after_exclamation() {
        assert_eq!(cleanup_text("hello! world"), "Hello! World.");
    }

    #[test]
    fn test_no_capital_inside_numbers_and_urls() {
        // Cas réel de l'historique : « 0.4.0 De version »
        assert_eq!(
            cleanup_text("on est à la 0.4.0 de version et moi à la 0.3 sur mon ordinateur"),
            "On est à la 0.4.0 de version et moi à la 0.3 sur mon ordinateur."
        );
        assert_eq!(cleanup_text("va sur google.com puis"), "Va sur google.com puis.");
    }

    #[test]
    fn test_capitalize_after_ellipsis() {
        assert_eq!(cleanup_text("bon... alors"), "Bon... Alors.");
    }

    #[test]
    fn test_ensure_ending_punctuation() {
        assert_eq!(cleanup_text("hello world"), "Hello world.");
    }

    #[test]
    fn test_preserve_existing_ending_punctuation() {
        assert_eq!(cleanup_text("hello world."), "Hello world.");
        assert_eq!(cleanup_text("hello world!"), "Hello world!");
        assert_eq!(cleanup_text("hello world?"), "Hello world?");
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(cleanup_text(""), "");
        assert_eq!(cleanup_text("   "), "");
    }

    #[test]
    fn test_already_clean() {
        assert_eq!(cleanup_text("Hello world."), "Hello world.");
    }
}
