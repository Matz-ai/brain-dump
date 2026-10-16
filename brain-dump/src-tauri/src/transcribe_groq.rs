use reqwest::multipart;
use std::path::PathBuf;

pub async fn transcribe_groq(
    api_key: &str,
    audio_path: &PathBuf,
    language: &str,
    model: &str,
    prompt: &str,
) -> Result<String, String> {
    if api_key.is_empty() {
        return Err("Groq API key not set. Please enter your API key in settings.".to_string());
    }

    let audio_bytes = std::fs::read(audio_path)
        .map_err(|e| format!("Failed to read audio file: {}", e))?;

    let file_part = multipart::Part::bytes(audio_bytes)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;

    let mut form = multipart::Form::new()
        .text("model", model.to_string())
        .text("language", language.to_string())
        .text("response_format", "verbose_json")
        .text("temperature", "0")
        .part("file", file_part);

    if !prompt.trim().is_empty() {
        form = form.text("prompt", prompt.to_string());
    }

    let client = reqwest::Client::new();
    let response = client
        .post("https://api.groq.com/openai/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Groq API request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Groq API error ({}): {}", status, body));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Groq response: {}", e))?;

    extract_text(&json)
}

/// Rebuilds the transcript from segments, dropping the ones Whisper made up:
/// silence it was unsure about, repetition loops, and its classic subtitle credits.
fn extract_text(json: &serde_json::Value) -> Result<String, String> {
    let Some(segments) = json["segments"].as_array() else {
        let text = json["text"]
            .as_str()
            .ok_or("No 'text' field in Groq response".to_string())?;
        return Ok(if is_hallucination(text) { String::new() } else { text.to_string() });
    };

    let kept: Vec<&str> = segments
        .iter()
        .filter(|seg| {
            let no_speech = seg["no_speech_prob"].as_f64().unwrap_or(0.0);
            let logprob = seg["avg_logprob"].as_f64().unwrap_or(0.0);
            let compression = seg["compression_ratio"].as_f64().unwrap_or(0.0);
            // Same thresholds as OpenAI's reference implementation
            !(no_speech > 0.6 && logprob < -1.0) && compression <= 2.4
        })
        .filter_map(|seg| seg["text"].as_str())
        .filter(|text| !is_hallucination(text))
        .collect();

    Ok(kept.join(" "))
}

/// Phrases Whisper hallucinates from its YouTube/TV subtitle training data.
const HALLUCINATION_FRAGMENTS: &[&str] = &["sous-titr", "sous titr", "amara.org"];
const HALLUCINATION_SENTENCES: &[&str] = &[
    "merci d'avoir regardé",
    "merci d'avoir regardé cette vidéo",
    "abonnez-vous",
    "n'oubliez pas de vous abonner",
    "thanks for watching",
    "thank you for watching",
    "you",
];

fn is_hallucination(segment: &str) -> bool {
    let lower = segment.to_lowercase().replace('’', "'");
    if HALLUCINATION_FRAGMENTS.iter().any(|f| lower.contains(f)) {
        return true;
    }
    let bare = lower.trim_matches(|c: char| !c.is_alphanumeric()).trim();
    HALLUCINATION_SENTENCES.contains(&bare)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_api_key() {
        let path = PathBuf::from("/tmp/test.wav");
        let result = transcribe_groq("", &path, "fr", "whisper-large-v3-turbo", "").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("API key not set"));
    }

    fn seg(text: &str, no_speech: f64, logprob: f64) -> serde_json::Value {
        serde_json::json!({
            "text": text, "no_speech_prob": no_speech,
            "avg_logprob": logprob, "compression_ratio": 1.2
        })
    }

    #[test]
    fn test_keeps_real_speech() {
        let json = serde_json::json!({ "segments": [
            seg(" Bonjour, je teste la dictée.", 0.01, -0.2),
            seg(" Ça marche bien.", 0.02, -0.3),
        ]});
        assert_eq!(
            extract_text(&json).unwrap(),
            " Bonjour, je teste la dictée.  Ça marche bien."
        );
    }

    #[test]
    fn test_drops_subtitle_credits() {
        let json = serde_json::json!({ "segments": [
            seg(" Envoie le mail à Paul.", 0.01, -0.2),
            seg(" Sous-titrage Société Radio-Canada", 0.1, -0.4),
            seg(" Sous-titres réalisés par la communauté d'Amara.org", 0.1, -0.4),
        ]});
        assert_eq!(extract_text(&json).unwrap(), " Envoie le mail à Paul.");
    }

    #[test]
    fn test_drops_unsure_silence_segment() {
        let json = serde_json::json!({ "segments": [
            seg(" Note pour demain.", 0.05, -0.3),
            seg(" Et voilà.", 0.9, -1.4),
        ]});
        assert_eq!(extract_text(&json).unwrap(), " Note pour demain.");
    }

    #[test]
    fn test_hallucination_sentences_only_match_whole_segment() {
        assert!(is_hallucination(" Merci d'avoir regardé !"));
        assert!(is_hallucination("Thanks for watching."));
        assert!(!is_hallucination("Merci à tous."));
        assert!(!is_hallucination(" Merci d'avoir regardé le dossier de Paul."));
        assert!(!is_hallucination(" J'ai regardé Radio-Canada hier."));
    }

    #[test]
    fn test_plain_json_fallback() {
        let json = serde_json::json!({ "text": "Bonjour" });
        assert_eq!(extract_text(&json).unwrap(), "Bonjour");
    }
}
