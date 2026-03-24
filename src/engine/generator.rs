use super::EngineError;
use rig::client::ProviderClient;
use rig::providers::gemini;

/// Wrapper around the LLM client
pub struct LlmClient {
    client: gemini::Client,
    model: String,
}

impl LlmClient {
    /// Create client from GEMINI_API_KEY env var
    pub fn from_env(model: impl Into<String>) -> Result<Self, EngineError> {
        let client = gemini::Client::from_env();
        Ok(Self {
            client,
            model: model.into(),
        })
    }
}

/// Generate a response from the LLM
pub async fn generate(prompt: &str, client: &LlmClient) -> Result<String, EngineError> {
    use rig::client::CompletionClient;
    use rig::completion::Prompt;

    let agent = client.client.agent(&client.model).build();

    agent
        .prompt(prompt)
        .await
        .map_err(|e| EngineError::Generation(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests require API key, mark as ignored
    #[tokio::test]
    #[ignore = "requires GEMINI_API_KEY"]
    async fn test_generate_basic() {
        let client = LlmClient::from_env("gemini-3-flash-preview").unwrap();
        let response = generate("Say 'hello' and nothing else.", &client)
            .await
            .unwrap();

        assert!(response.to_lowercase().contains("hello"));
    }
}
