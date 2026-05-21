use async_trait::async_trait;
use code_rag_store::LlmError;
use code_rag_store::seams::LlmClient;
use rig::client::ProviderClient;
use rig::providers::gemini;

/// Concrete `LlmClient` seam impl backed by rig-core's Gemini provider.
pub struct RigGeminiImpl {
    client: gemini::Client,
    model: String,
}

impl RigGeminiImpl {
    /// Create client from `GEMINI_API_KEY` env var.
    pub fn from_env(model: impl Into<String>) -> anyhow::Result<Self> {
        let client = gemini::Client::from_env();
        Ok(Self {
            client,
            model: model.into(),
        })
    }
}

#[async_trait]
impl LlmClient for RigGeminiImpl {
    async fn generate(&self, prompt: &str) -> Result<String, LlmError> {
        use rig::client::CompletionClient;
        use rig::completion::Prompt;

        let agent = self.client.agent(&self.model).build();
        agent
            .prompt(prompt)
            .await
            .map_err(|e| LlmError::Generation(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests require API key, mark as ignored
    #[tokio::test]
    #[ignore = "requires GEMINI_API_KEY"]
    async fn test_generate_basic() {
        let client = RigGeminiImpl::from_env("gemini-3.1-flash-lite").unwrap();
        let response = client
            .generate("Say 'hello' and nothing else.")
            .await
            .unwrap();

        assert!(response.to_lowercase().contains("hello"));
    }
}
