pub mod ai;
pub mod openai;

pub use ai::{AiProvider, ChatRequest, ProviderError};
pub use openai::OpenAiClient;

#[cfg(any(test, feature = "test-support"))]
pub use ai::FakeAiProvider;
