use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompanionEmotion {
    Idle,
    Happy,
    Attentive,
    Vibing,
    Surprise,
    Guilty,
    Sad,
    Thinking,
    Speaking,
}

impl CompanionEmotion {
    pub fn from_tag(tag: &str) -> Self {
        let low = tag.to_lowercase();
        if low.contains("happy") || low.contains("smile") || low.contains("laugh") || low.contains("clap") {
            CompanionEmotion::Happy
        } else if low.contains("attentive") || low.contains("listen") || low.contains("nod") {
            CompanionEmotion::Attentive
        } else if low.contains("vibing") || low.contains("hum") || low.contains("chill") {
            CompanionEmotion::Vibing
        } else if low.contains("surprise") || low.contains("shock") || low.contains("gasp") || low.contains("pout") {
            CompanionEmotion::Surprise
        } else if low.contains("guilty") || low.contains("shy") || low.contains("blush") || low.contains("nervous") {
            CompanionEmotion::Guilty
        } else if low.contains("sad") || low.contains("cry") || low.contains("sigh") {
            CompanionEmotion::Sad
        } else if low.contains("think") {
            CompanionEmotion::Thinking
        } else {
            CompanionEmotion::Idle
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            CompanionEmotion::Idle => "Idle",
            CompanionEmotion::Happy => "Happy",
            CompanionEmotion::Attentive => "Attentive",
            CompanionEmotion::Vibing => "Vibing",
            CompanionEmotion::Surprise => "Surprise",
            CompanionEmotion::Guilty => "Blushing",
            CompanionEmotion::Sad => "Sad",
            CompanionEmotion::Thinking => "Thinking",
            CompanionEmotion::Speaking => "Speaking",
        }
    }

    #[allow(dead_code)]
    pub fn emoji(&self) -> &'static str {
        match self {
            CompanionEmotion::Idle => "🦐",
            CompanionEmotion::Happy => "✨",
            CompanionEmotion::Attentive => "👂",
            CompanionEmotion::Vibing => "🎵",
            CompanionEmotion::Surprise => "😮",
            CompanionEmotion::Guilty => "😳",
            CompanionEmotion::Sad => "💧",
            CompanionEmotion::Thinking => "💭",
            CompanionEmotion::Speaking => "🗣️",
        }
    }

    #[allow(dead_code)]
    pub fn symbol(&self) -> &'static str {
        match self {
            CompanionEmotion::Idle => "●",
            CompanionEmotion::Happy => "✦",
            CompanionEmotion::Attentive => "▲",
            CompanionEmotion::Vibing => "~",
            CompanionEmotion::Surprise => "!",
            CompanionEmotion::Guilty => "*",
            CompanionEmotion::Sad => "v",
            CompanionEmotion::Thinking => "...",
            CompanionEmotion::Speaking => "►",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: MessageRole,
    pub text: String,
    pub emotion: Option<CompanionEmotion>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub expires_at: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LlmStatus {
    Starting,
    LoadingModel,
    Ready,
    Error(String),
}

impl LlmStatus {
    pub fn label(&self) -> String {
        match self {
            LlmStatus::Starting => "Bonsai 27B: Starting...".to_string(),
            LlmStatus::LoadingModel => "Bonsai 27B: Loading model into VRAM...".to_string(),
            LlmStatus::Ready => "Bonsai 27B: Ready".to_string(),
            LlmStatus::Error(err) => format!("Bonsai 27B Error: {err}"),
        }
    }
}
