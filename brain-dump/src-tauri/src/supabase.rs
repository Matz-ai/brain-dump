use reqwest::Client;
use serde_json::json;

pub async fn insert_note(
    supabase_url: &str,
    anon_key: &str,
    transcript: &str,
    source: &str,
    context: Option<serde_json::Value>,
) -> Result<(), String> {
    if supabase_url.is_empty() || anon_key.is_empty() {
        return Err("Supabase not configured".to_string());
    }

    let url = format!("{}/rest/v1/notes", supabase_url.trim_end_matches('/'));
    let body = json!({
        "source": source,
        "transcript": transcript,
        "context": context.unwrap_or(json!({}))
    });

    let client = Client::new();
    let response = client
        .post(&url)
        .header("apikey", anon_key)
        .header("Authorization", format!("Bearer {}", anon_key))
        .header("Content-Type", "application/json")
        .header("Prefer", "return=minimal")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Supabase request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let err_body = response.text().await.unwrap_or_default();
        return Err(format!("Supabase error ({}): {}", status, err_body));
    }

    println!("[brain-dump] Note inserted in Supabase ({})", source);
    Ok(())
}
