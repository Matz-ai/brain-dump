use std::time::Duration;

/// Free tier, same limits as qwen (8k tokens/min) but noticeably better at rewriting and translating.
const MODEL: &str = "openai/gpt-oss-120b";
/// Rewriting a long selection takes longer than a dictation cleanup.
const TIMEOUT: Duration = Duration::from_secs(8);
/// Free tier allows 8k tokens/min, input and output included: past ~8k chars the call would fail anyway.
pub const MAX_SELECTION_CHARS: usize = 8000;

const SYSTEM_PROMPT: &str = "Tu es un outil d'édition de texte intégré à une app de dictée. L'utilisateur a sélectionné un texte et dicte une instruction à voix haute.
Applique l'instruction au texte sélectionné et réponds uniquement avec le texte final, prêt à être collé à la place de la sélection : pas de guillemets autour, pas de préambule, pas d'explication, pas de bloc de code Markdown.
Sauf si l'instruction le demande, garde la langue, le ton et la mise en forme du texte (retours à la ligne, listes).
S'il n'y a pas de texte sélectionné, rédige le texte demandé par l'instruction, prêt à être inséré au curseur.
L'instruction vient d'une transcription vocale : elle peut contenir des fautes ou des hésitations, interprète-la au mieux.";

fn user_message(instruction: &str, selection: &str) -> String {
    if selection.trim().is_empty() {
        format!("Instruction : {}\n\n(Aucun texte sélectionné.)", instruction.trim())
    } else {
        format!(
            "Instruction : {}\n\nTexte sélectionné :\n<<<\n{}\n>>>",
            instruction.trim(),
            selection
        )
    }
}

/// Applies a spoken instruction to the selected text (or writes new text if nothing is selected).
/// Errors are short French messages meant for the overlay.
pub async fn run_command(api_key: &str, instruction: &str, selection: &str) -> Result<String, String> {
    if selection.chars().count() > MAX_SELECTION_CHARS {
        return Err(format!("Sélection trop longue (max {} caractères)", MAX_SELECTION_CHARS));
    }

    let body = serde_json::json!({
        "model": MODEL,
        "temperature": 0.3,
        "reasoning_effort": "low",
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": user_message(instruction, selection) },
        ],
    });

    let response = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?
        .post("https://api.groq.com/openai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            eprintln!("[brain-dump] Command request failed: {}", e);
            if e.is_timeout() { "L'IA a mis trop de temps" } else { "Pas de connexion à Groq" }.to_string()
        })?;

    let status = response.status();
    if status.as_u16() == 429 {
        let header = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = response.text().await.unwrap_or_default();
        eprintln!("[brain-dump] Command rate limited: {}", body);
        return Err(rate_limit_message(header.as_deref(), &body));
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        eprintln!("[brain-dump] Command error ({}): {}", status, body);
        return Err(format!("Erreur Groq ({})", status.as_u16()));
    }

    let json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    let result = json["choices"][0]["message"]["content"].as_str().unwrap_or_default();
    let result = clean_result(result);
    if result.is_empty() {
        return Err("L'IA n'a rien renvoyé".to_string());
    }
    Ok(result)
}

/// Groq sends `retry-after` in seconds, and "Please try again in 7.66s" (or "1m2.5s") in the body.
fn rate_limit_message(retry_after: Option<&str>, body: &str) -> String {
    let secs = retry_after
        .and_then(|v| v.trim().parse::<f64>().ok())
        .or_else(|| {
            let rest = body.split("try again in ").nth(1)?;
            let end = rest
                .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == 'm' || c == 's'))
                .unwrap_or(rest.len());
            let spec = rest[..end].trim_end_matches('.');
            let (mins, secs) = spec.split_once('m').unwrap_or(("0", spec));
            Some(mins.parse::<f64>().ok()? * 60.0 + secs.trim_end_matches('s').parse::<f64>().unwrap_or(0.0))
        });
    match secs {
        Some(s) => format!("Limite gratuite atteinte, réessaie dans {} s", s.ceil().max(1.0) as u64),
        None => "Limite gratuite atteinte, réessaie plus tard".to_string(),
    }
}

/// Strips what models add around the answer despite the prompt: a code fence, wrapping quotes,
/// Markdown hard breaks (trailing double spaces) and narrow no-break spaces (like correct_groq).
fn clean_result(text: &str) -> String {
    let mut text = text.trim();
    if let Some(inner) = text.strip_prefix("```") {
        let inner = inner.split_once('\n').map_or("", |(_, rest)| rest);
        text = inner.trim_end().strip_suffix("```").unwrap_or(inner).trim();
    }
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') && text.matches('"').count() == 2 {
        text = &text[1..text.len() - 1];
    }
    text.lines()
        .map(|l| l.trim_end().replace('\u{202F}', " "))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_with_selection() {
        let msg = user_message(" traduis en anglais ", "Merci,\nà demain");
        assert_eq!(
            msg,
            "Instruction : traduis en anglais\n\nTexte sélectionné :\n<<<\nMerci,\nà demain\n>>>"
        );
    }

    #[test]
    fn test_prompt_without_selection() {
        for empty in ["", "  \n"] {
            let msg = user_message("écris un mail à Julie", empty);
            assert_eq!(msg, "Instruction : écris un mail à Julie\n\n(Aucun texte sélectionné.)");
        }
    }

    #[tokio::test]
    async fn test_rejects_long_selection_without_calling_api() {
        let long = "a".repeat(MAX_SELECTION_CHARS + 1);
        let err = run_command("no-key", "résume", &long).await.unwrap_err();
        assert!(err.starts_with("Sélection trop longue"));
    }

    #[test]
    fn test_rate_limit_message() {
        assert_eq!(
            rate_limit_message(Some("12"), ""),
            "Limite gratuite atteinte, réessaie dans 12 s"
        );
        let body = r#"{"error":{"message":"Rate limit reached for model `openai/gpt-oss-120b` on tokens per minute (TPM): Limit 8000, Used 7900. Please try again in 7.66s. Visit https://console.groq.com"}}"#;
        assert_eq!(
            rate_limit_message(None, body),
            "Limite gratuite atteinte, réessaie dans 8 s"
        );
        let body = "Please try again in 1m2.5s.";
        assert_eq!(
            rate_limit_message(None, body),
            "Limite gratuite atteinte, réessaie dans 63 s"
        );
        assert_eq!(
            rate_limit_message(None, "?"),
            "Limite gratuite atteinte, réessaie plus tard"
        );
    }

    #[test]
    fn test_clean_result() {
        assert_eq!(clean_result("```\n- pain\n- lait\n```"), "- pain\n- lait");
        assert_eq!(clean_result("```markdown\nHello\n```"), "Hello");
        assert_eq!(clean_result("\"Thanks!\""), "Thanks!");
        assert_eq!(clean_result("- pain  \n- lait  "), "- pain\n- lait");
        assert_eq!(clean_result("Il a dit \"oui\" puis \"non\""), "Il a dit \"oui\" puis \"non\"");
        assert_eq!(clean_result("Merci\u{202F}!"), "Merci !");
    }
}
