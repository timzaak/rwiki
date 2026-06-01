use serde::{Deserialize, Serialize};
use std::time::Instant;

/// 默认会话 TTL（1 小时）
pub const SESSION_TTL_SECS: u64 = 3600;

/// 聊天消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// 聊天会话（纯内存，不持久化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub messages: Vec<ChatMessage>,
    /// 压缩后的历史摘要（由 LLM 生成，替代已被压缩的旧消息）
    #[serde(default)]
    pub summary: Option<String>,
    /// 上次访问时间（不序列化，内存生命周期用途）
    #[serde(skip, default = "Instant::now")]
    pub last_accessed: Instant,
}

impl ChatSession {
    pub fn new(id: String) -> Self {
        Self {
            id,
            messages: Vec::new(),
            summary: None,
            last_accessed: Instant::now(),
        }
    }

    pub fn add_message(&mut self, role: impl Into<String>, content: impl Into<String>) {
        self.messages.push(ChatMessage {
            role: role.into(),
            content: content.into(),
        });
        self.last_accessed = Instant::now();
    }

    pub fn get_history(&self) -> &[ChatMessage] {
        &self.messages
    }

    pub fn touch(&mut self) {
        self.last_accessed = Instant::now();
    }

    /// 检查会话是否已过期
    pub fn is_expired(&self, ttl_secs: u64) -> bool {
        self.last_accessed.elapsed().as_secs() > ttl_secs
    }

    /// 返回最后 `window_size` 条消息。
    /// 若消息总数不足，返回全部消息。
    pub fn get_sliding_window(&self, window_size: usize) -> &[ChatMessage] {
        if self.messages.len() <= window_size {
            &self.messages
        } else {
            &self.messages[self.messages.len() - window_size..]
        }
    }

    /// 估算当前会话的 token 数。
    /// 公式：(summary 字符数 + 所有消息字符数) * 3 / 5
    /// 使用 `.chars().count()` 以正确处理中英混合文本。
    /// 系数 3/5 (= 1.67 chars/token) 包含约 20% 安全余量。
    pub fn estimate_tokens(&self) -> usize {
        let char_count: usize = self
            .summary
            .as_ref()
            .map(|s| s.chars().count())
            .unwrap_or(0)
            + self
                .messages
                .iter()
                .map(|m| m.content.chars().count())
                .sum::<usize>();
        char_count * 3 / 5
    }

    /// 判断是否需要压缩历史。
    /// 当 (消息数 > threshold OR 估算 token 数 > token_budget)
    /// 且 消息数 > sliding_window_size（有窗口外的消息可压缩）时返回 true。
    pub fn should_compact(
        &self,
        threshold: usize,
        token_budget: usize,
        sliding_window_size: usize,
    ) -> bool {
        let needs_compact =
            self.messages.len() > threshold || self.estimate_tokens() > token_budget;
        let has_messages_outside_window = self.messages.len() > sliding_window_size;
        needs_compact && has_messages_outside_window
    }

    /// 压缩历史：保留最后 `window_size` 条消息，设置摘要。
    /// 调用方负责在调用前通过 LLM 合并旧摘要与旧消息生成新摘要。
    pub fn compact_history(&mut self, summary: String, window_size: usize) {
        if self.messages.len() > window_size {
            self.messages = self.messages.split_off(self.messages.len() - window_size);
        }
        self.summary = Some(summary);
    }
}

/// 从 HashMap 中移除所有过期会话
pub fn evict_expired_sessions(sessions: &mut std::collections::HashMap<String, ChatSession>) {
    sessions.retain(|_, session| !session.is_expired(SESSION_TTL_SECS));
}
