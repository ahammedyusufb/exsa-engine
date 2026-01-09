use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMessage {
    pub role: String,
    pub content: String,
    pub tokens: usize,
    pub timestamp: u64,
    pub importance: MessageImportance,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessageImportance {
    System = 3,
    Critical = 2,
    Normal = 1,
    Ephemeral = 0,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextUsage {
    pub current_tokens: usize,
    pub max_tokens: usize,
    pub message_count: usize,
    pub usage_percent: f32,
}

impl ContextUsage {
    pub fn new(current: usize, max: usize, count: usize) -> Self {
        Self {
            current_tokens: current,
            max_tokens: max,
            message_count: count,
            usage_percent: (current as f32 / max as f32) * 100.0,
        }
    }

    pub fn should_trim(&self) -> bool {
        self.usage_percent > 75.0
    }

    pub fn is_critical(&self) -> bool {
        self.usage_percent > 90.0
    }
}

pub struct ContextWindowManager {
    messages: VecDeque<ContextMessage>,
    context_size: usize,
    min_response_tokens: usize,
    system_prompt: Option<ContextMessage>,
    current_token_count: usize,
}

impl ContextWindowManager {
    pub fn new(context_size: usize) -> Self {
        Self {
            messages: VecDeque::new(),
            context_size,
            min_response_tokens: 512,
            system_prompt: None,
            current_token_count: 0,
        }
    }

    pub fn with_min_response_tokens(mut self, tokens: usize) -> Self {
        self.min_response_tokens = tokens;
        self
    }

    pub fn set_system_prompt(&mut self, content: String) {
        let tokens = Self::estimate_tokens(&content);
        self.system_prompt = Some(ContextMessage {
            role: "system".to_string(),
            content,
            tokens,
            timestamp: Self::current_timestamp(),
            importance: MessageImportance::System,
        });
        self.recalculate_tokens();
    }

    pub fn add_message(&mut self, role: String, content: String, importance: MessageImportance) {
        let tokens = Self::estimate_tokens(&content);
        let message = ContextMessage {
            role,
            content,
            tokens,
            timestamp: Self::current_timestamp(),
            importance,
        };

        self.messages.push_back(message);
        self.current_token_count += tokens;

        self.auto_trim_if_needed();
    }

    fn auto_trim_if_needed(&mut self) {
        let usage = self.get_usage();

        if usage.should_trim() {
            self.smart_trim();
        }
    }

    fn smart_trim(&mut self) {
        let target = (self.context_size as f32 * 0.6) as usize;
        let _system_tokens = self.system_prompt.as_ref().map(|m| m.tokens).unwrap_or(0);

        while self.current_token_count > target && self.messages.len() > 1 {
            if let Some(msg) = self.find_least_important_message() {
                let idx = msg;
                if let Some(removed) = self.messages.remove(idx) {
                    self.current_token_count -= removed.tokens;
                }
            } else {
                break;
            }
        }
    }

    fn find_least_important_message(&self) -> Option<usize> {
        let mut min_importance = MessageImportance::System;
        let mut oldest_idx = None;
        let mut oldest_time = u64::MAX;

        for (idx, msg) in self.messages.iter().enumerate() {
            if msg.importance == MessageImportance::System
                || msg.importance == MessageImportance::Critical
            {
                continue;
            }

            if msg.importance < min_importance
                || (msg.importance == min_importance && msg.timestamp < oldest_time)
            {
                min_importance = msg.importance;
                oldest_time = msg.timestamp;
                oldest_idx = Some(idx);
            }
        }

        oldest_idx
    }

    pub fn get_messages_for_api(&self) -> Vec<ContextMessage> {
        let mut messages = Vec::new();

        if let Some(sys_msg) = &self.system_prompt {
            messages.push(sys_msg.clone());
        }

        messages.extend(self.messages.iter().cloned());
        messages
    }

    pub fn get_usage(&self) -> ContextUsage {
        let system_tokens = self.system_prompt.as_ref().map(|m| m.tokens).unwrap_or(0);
        let total = system_tokens + self.current_token_count;

        ContextUsage::new(total, self.context_size, self.messages.len())
    }

    pub fn available_tokens(&self) -> usize {
        let used =
            self.current_token_count + self.system_prompt.as_ref().map(|m| m.tokens).unwrap_or(0);

        if used + self.min_response_tokens >= self.context_size {
            0
        } else {
            (self.context_size - used).saturating_sub(self.min_response_tokens)
        }
    }

    pub fn can_add_tokens(&self, tokens: usize) -> bool {
        self.available_tokens() >= tokens
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.current_token_count = 0;
    }

    pub fn clear_all(&mut self) {
        self.clear();
        self.system_prompt = None;
    }

    fn recalculate_tokens(&mut self) {
        self.current_token_count = self.messages.iter().map(|m| m.tokens).sum();
    }

    fn estimate_tokens(text: &str) -> usize {
        (text.len() / 4).max(1)
    }

    fn current_timestamp() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    pub fn trim_to_fit(&mut self, required_tokens: usize) -> bool {
        while !self.can_add_tokens(required_tokens) && !self.messages.is_empty() {
            if let Some(idx) = self.find_least_important_message() {
                if let Some(removed) = self.messages.remove(idx) {
                    self.current_token_count -= removed.tokens;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        self.can_add_tokens(required_tokens)
    }

    pub fn get_recent_messages(&self, count: usize) -> Vec<ContextMessage> {
        self.messages
            .iter()
            .rev()
            .take(count)
            .rev()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_usage() {
        let usage = ContextUsage::new(3000, 4096, 10);
        assert!(!usage.should_trim());

        let usage2 = ContextUsage::new(3200, 4096, 15);
        assert!(usage2.should_trim());
    }

    #[test]
    fn test_message_priority() {
        let mut manager = ContextWindowManager::new(1000);
        manager.add_message(
            "user".to_string(),
            "test".to_string(),
            MessageImportance::Normal,
        );
        assert_eq!(manager.messages.len(), 1);
    }
}
