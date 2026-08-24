use serde::{Deserialize, Serialize};

// ### REQUEST

#[derive(Serialize, Debug)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: Vec<ChatMessage<'a>>,
    // both servers default to false. sent explicitly anyway: it is the one
    // field a later streaming stage flips, and implicit is not self-documenting
    pub stream: bool,
    pub temperature: f32,
    // NOT max_completion_tokens - LM Studio takes max_tokens and ollama's
    // compat layer maps it to num_predict; the newer name is not honoured there
    pub max_tokens: u32,

    // How a reasoning model is actually told not to reason.
    //
    // The prompt convention - "/no_think" in the system message - is a Qwen3
    // habit that Qwen3.5 does not keep: measured, the model produced 406
    // reasoning tokens and an empty answer with that directive in place. The
    // switch that works goes in the request, where llama.cpp, LM Studio and
    // vLLM all hand it to the chat template.
    //
    // Omitted entirely unless thinking is being turned off, so a server that
    // has never heard of the field only meets it when someone asks for that.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_template_kwargs: Option<ChatTemplateKwargs>,
}

#[derive(Serialize, Debug)]
pub struct ChatTemplateKwargs {
    pub enable_thinking: bool,
}

#[derive(Serialize, Debug)]
pub struct ChatMessage<'a> {
    pub role: &'a str,
    pub content: &'a str,
}

// ### RESPONSE
//
// no #[serde(deny_unknown_fields)] anywhere below. LM Studio adds `stats`,
// `model_info` and `runtime` in some builds, and reasoning models add
// `reasoning_content` next to `content`. serde's default ignore is what we want.

#[derive(Deserialize, Debug)]
pub struct ChatResponse {
    #[serde(default)]
    pub model: String,
    // can legitimately be empty - never index [0]
    #[serde(default)]
    pub choices: Vec<Choice>,
    // ollama omits it in some versions
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Deserialize, Debug)]
pub struct Choice {
    #[serde(default)]
    pub message: Option<ChoiceMessage>,
    // LM Studio returns null here in some paths.
    // parsed but not consumed by stage 1 - it is what a later stage reads to
    // notice a "length" cut-off, and it is worth having in the Debug dump now
    #[allow(dead_code)]
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct ChoiceMessage {
    // null when a model emitted only reasoning or a tool call
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct Usage {
    #[serde(default)] pub prompt_tokens: u32,
    #[serde(default)] pub completion_tokens: u32,
    // redundant with the two above, kept so the Debug dump matches the wire
    #[allow(dead_code)]
    #[serde(default)] pub total_tokens: u32,
}

// ### ERROR BODIES
//
// LM Studio sends {"error":{"message":...,"code":"invalid_api_key"}} on auth
// but the bare string form {"error":"Model \"x\" not found. ..."} for a missing
// model. ollama sends the object form for both. accept either; the caller falls
// back to the raw body when neither parses, so the owner is never shown an
// empty reason.

#[derive(Deserialize, Debug)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum ErrorBody {
    // struct variant FIRST: untagged tries in order and Text would swallow
    // nothing here, but the order is load-bearing if a variant is ever added
    Structured { message: String },
    Text(String),
}

impl ErrorBody {
    pub fn message(&self) -> &str {
        match self {
            ErrorBody::Structured { message } => message,
            ErrorBody::Text(s) => s,
        }
    }
}
