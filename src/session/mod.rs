//! Session management for multi-user LLM inference
//!
//! Provides session-based context isolation, lifecycle management,
//! and fair resource allocation across concurrent users.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info};
use uuid::Uuid;

/// Session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Maximum idle time before session expires
    pub idle_timeout: Duration,
    /// Maximum total session lifetime
    pub max_lifetime: Duration,
    /// Maximum tokens to preserve for this session (n_keep)
    pub n_keep: usize,
    /// Maximum context tokens for this session
    pub max_context_tokens: usize,
    /// Enable prompt caching for this session
    pub enable_prompt_cache: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(300),  // 5 minutes
            max_lifetime: Duration::from_secs(3600), // 1 hour
            n_keep: 0,
            max_context_tokens: 4096,
            enable_prompt_cache: true,
        }
    }
}

/// Session state for lifecycle tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Session is active and processing
    Active,
    /// Session is idle but warm (cached KV)
    Idle,
    /// Session is suspended (can be resumed)
    Suspended,
    /// Session has expired
    Expired,
    /// Session was explicitly closed
    Closed,
}

/// Prompt cache entry for warm starts
#[derive(Debug, Clone)]
pub struct PromptCacheEntry {
    /// Hash of the cached prompt
    pub prompt_hash: u64,
    /// Number of tokens in cached prompt
    pub token_count: usize,
    /// KV cache position after processing this prompt
    pub kv_position: usize,
    /// Last access time
    pub last_used: Instant,
    /// Access count for LRU
    pub access_count: u64,
}

/// Individual user session with state and resource tracking
#[derive(Debug)]
pub struct Session {
    /// Unique session identifier
    pub id: Uuid,
    /// User identifier (optional)
    pub user_id: Option<String>,
    /// Session configuration
    pub config: SessionConfig,
    /// Current state
    pub state: SessionState,
    /// KV cache slot ID for this session
    pub kv_slot_id: Option<usize>,
    /// Current position in KV cache
    pub kv_position: usize,
    /// Total tokens generated in this session
    pub tokens_generated: usize,
    /// Total requests processed
    pub request_count: usize,
    /// Prompt cache for warm starts
    prompt_cache: HashMap<u64, PromptCacheEntry>,
    /// Session creation time
    pub created_at: Instant,
    /// Last activity time
    pub last_active: Instant,
    /// Last request completion time
    last_request_end: Option<Instant>,
    /// Cumulative generation time
    total_generation_time: Duration,
}

impl Session {
    /// Create a new session
    pub fn new(user_id: Option<String>, config: SessionConfig) -> Self {
        let now = Instant::now();
        Self {
            id: Uuid::new_v4(),
            user_id,
            config,
            state: SessionState::Active,
            kv_slot_id: None,
            kv_position: 0,
            tokens_generated: 0,
            request_count: 0,
            prompt_cache: HashMap::new(),
            created_at: now,
            last_active: now,
            last_request_end: None,
            total_generation_time: Duration::ZERO,
        }
    }

    /// Create session with specific ID (for resume)
    pub fn with_id(id: Uuid, user_id: Option<String>, config: SessionConfig) -> Self {
        let mut session = Self::new(user_id, config);
        session.id = id;
        session
    }

    /// Mark session as active (processing request)
    pub fn activate(&mut self) {
        self.state = SessionState::Active;
        self.last_active = Instant::now();
    }

    /// Mark session as idle (waiting for request)
    pub fn mark_idle(&mut self) {
        self.state = SessionState::Idle;
        self.last_request_end = Some(Instant::now());
    }

    /// Suspend session (free resources but allow resume)
    pub fn suspend(&mut self) {
        self.state = SessionState::Suspended;
        self.kv_slot_id = None;
        debug!("Session {} suspended", self.id);
    }

    /// Close session permanently
    pub fn close(&mut self) {
        self.state = SessionState::Closed;
        self.kv_slot_id = None;
        self.prompt_cache.clear();
        info!(
            "Session {} closed after {} requests, {} tokens",
            self.id, self.request_count, self.tokens_generated
        );
    }

    /// Check if session has expired
    pub fn is_expired(&self) -> bool {
        if self.state == SessionState::Expired || self.state == SessionState::Closed {
            return true;
        }

        let now = Instant::now();

        // Check max lifetime
        if now.duration_since(self.created_at) > self.config.max_lifetime {
            return true;
        }

        // Check idle timeout (only for idle sessions)
        if self.state == SessionState::Idle
            && now.duration_since(self.last_active) > self.config.idle_timeout
        {
            return true;
        }

        false
    }

    /// Record request completion
    pub fn record_request(&mut self, tokens: usize, duration: Duration) {
        self.request_count += 1;
        self.tokens_generated += tokens;
        self.total_generation_time += duration;
        self.last_active = Instant::now();
    }

    /// Get average tokens per second
    pub fn tokens_per_second(&self) -> f64 {
        if self.total_generation_time.as_secs_f64() > 0.0 {
            self.tokens_generated as f64 / self.total_generation_time.as_secs_f64()
        } else {
            0.0
        }
    }

    /// Session lifetime
    pub fn lifetime(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Time since last activity
    pub fn idle_time(&self) -> Duration {
        self.last_active.elapsed()
    }

    // ==================== PROMPT CACHING ====================

    /// Cache a prompt for warm starts
    pub fn cache_prompt(&mut self, prompt_hash: u64, token_count: usize, kv_position: usize) {
        if !self.config.enable_prompt_cache {
            return;
        }

        let entry = PromptCacheEntry {
            prompt_hash,
            token_count,
            kv_position,
            last_used: Instant::now(),
            access_count: 1,
        };

        self.prompt_cache.insert(prompt_hash, entry);
        debug!(
            "Cached prompt hash={} at kv_pos={}",
            prompt_hash, kv_position
        );
    }

    /// Get cached prompt position for warm start
    pub fn get_cached_prompt(&mut self, prompt_hash: u64) -> Option<usize> {
        if let Some(entry) = self.prompt_cache.get_mut(&prompt_hash) {
            entry.last_used = Instant::now();
            entry.access_count += 1;
            debug!(
                "Prompt cache hit: hash={}, kv_pos={}",
                prompt_hash, entry.kv_position
            );
            Some(entry.kv_position)
        } else {
            None
        }
    }

    /// Clear old prompt cache entries
    pub fn cleanup_prompt_cache(&mut self, max_age: Duration) {
        let now = Instant::now();
        self.prompt_cache
            .retain(|_, entry| now.duration_since(entry.last_used) < max_age);
    }

    /// Get n_keep value for this session
    pub fn n_keep(&self) -> usize {
        self.config.n_keep
    }

    /// Convert to sampling params extension
    pub fn to_params_extension(&self) -> (Option<usize>, Option<String>) {
        (Some(self.config.n_keep), Some(self.id.to_string()))
    }
}

/// Session statistics for monitoring
#[derive(Debug, Clone, Serialize)]
pub struct SessionStats {
    pub session_id: Uuid,
    pub user_id: Option<String>,
    pub state: SessionState,
    pub kv_slot_id: Option<usize>,
    pub kv_position: usize,
    pub tokens_generated: usize,
    pub request_count: usize,
    pub tokens_per_second: f64,
    pub lifetime_secs: f64,
    pub idle_secs: f64,
    pub prompt_cache_entries: usize,
}

impl From<&Session> for SessionStats {
    fn from(session: &Session) -> Self {
        Self {
            session_id: session.id,
            user_id: session.user_id.clone(),
            state: session.state,
            kv_slot_id: session.kv_slot_id,
            kv_position: session.kv_position,
            tokens_generated: session.tokens_generated,
            request_count: session.request_count,
            tokens_per_second: session.tokens_per_second(),
            lifetime_secs: session.lifetime().as_secs_f64(),
            idle_secs: session.idle_time().as_secs_f64(),
            prompt_cache_entries: session.prompt_cache.len(),
        }
    }
}

/// Session manager for multi-user support
pub struct SessionManager {
    /// Active sessions by ID
    sessions: HashMap<Uuid, Session>,
    /// User ID to session ID mapping
    user_sessions: HashMap<String, Uuid>,
    /// Default session configuration
    default_config: SessionConfig,
    /// Maximum concurrent sessions
    max_sessions: usize,
    /// Total sessions created
    total_created: usize,
    /// Total sessions expired
    total_expired: usize,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new(max_sessions: usize) -> Self {
        info!(
            "🔐 Initializing SessionManager: max_sessions={}",
            max_sessions
        );
        Self {
            sessions: HashMap::new(),
            user_sessions: HashMap::new(),
            default_config: SessionConfig::default(),
            max_sessions,
            total_created: 0,
            total_expired: 0,
        }
    }

    /// Create a new session
    pub fn create_session(
        &mut self,
        user_id: Option<String>,
        config: Option<SessionConfig>,
    ) -> Result<Uuid, String> {
        // Check capacity
        if self.sessions.len() >= self.max_sessions {
            // Try to evict expired sessions first
            self.cleanup_expired();

            if self.sessions.len() >= self.max_sessions {
                return Err("Maximum sessions reached".to_string());
            }
        }

        let config = config.unwrap_or_else(|| self.default_config.clone());
        let session = Session::new(user_id.clone(), config);
        let session_id = session.id;

        // Track user mapping
        if let Some(ref uid) = user_id {
            self.user_sessions.insert(uid.clone(), session_id);
        }

        self.sessions.insert(session_id, session);
        self.total_created += 1;

        info!("Created session {}: user={:?}", session_id, user_id);
        Ok(session_id)
    }

    /// Get session by ID
    pub fn get_session(&self, session_id: Uuid) -> Option<&Session> {
        self.sessions.get(&session_id)
    }

    /// Get mutable session by ID
    pub fn get_session_mut(&mut self, session_id: Uuid) -> Option<&mut Session> {
        self.sessions.get_mut(&session_id)
    }

    /// Get or create session for user
    pub fn get_or_create_for_user(&mut self, user_id: &str) -> Result<Uuid, String> {
        // Check for existing session
        if let Some(&session_id) = self.user_sessions.get(user_id) {
            if let Some(session) = self.sessions.get_mut(&session_id) {
                if !session.is_expired() {
                    session.activate();
                    return Ok(session_id);
                }
            }
        }

        // Create new session
        self.create_session(Some(user_id.to_string()), None)
    }

    /// Close a session
    pub fn close_session(&mut self, session_id: Uuid) -> bool {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.close();

            // Remove user mapping
            if let Some(ref user_id) = session.user_id {
                self.user_sessions.remove(user_id);
            }

            true
        } else {
            false
        }
    }

    /// Remove session completely
    pub fn remove_session(&mut self, session_id: Uuid) -> Option<Session> {
        if let Some(session) = self.sessions.remove(&session_id) {
            if let Some(ref user_id) = session.user_id {
                self.user_sessions.remove(user_id);
            }
            Some(session)
        } else {
            None
        }
    }

    /// Clean up expired sessions
    pub fn cleanup_expired(&mut self) -> usize {
        let expired: Vec<Uuid> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.is_expired())
            .map(|(id, _)| *id)
            .collect();

        let count = expired.len();

        for id in expired {
            self.remove_session(id);
            self.total_expired += 1;
        }

        if count > 0 {
            info!("Cleaned up {} expired sessions", count);
        }

        count
    }

    /// Get all session statistics
    pub fn all_stats(&self) -> Vec<SessionStats> {
        self.sessions.values().map(SessionStats::from).collect()
    }

    /// Get active session count
    pub fn active_count(&self) -> usize {
        self.sessions
            .values()
            .filter(|s| s.state == SessionState::Active)
            .count()
    }

    /// Get idle session count
    pub fn idle_count(&self) -> usize {
        self.sessions
            .values()
            .filter(|s| s.state == SessionState::Idle)
            .count()
    }

    /// Get total session count
    pub fn total_count(&self) -> usize {
        self.sessions.len()
    }

    /// Manager statistics
    pub fn manager_stats(&self) -> SessionManagerStats {
        SessionManagerStats {
            active_sessions: self.active_count(),
            idle_sessions: self.idle_count(),
            total_sessions: self.total_count(),
            max_sessions: self.max_sessions,
            total_created: self.total_created,
            total_expired: self.total_expired,
        }
    }
}

/// Session manager statistics
#[derive(Debug, Clone, Serialize)]
pub struct SessionManagerStats {
    pub active_sessions: usize,
    pub idle_sessions: usize,
    pub total_sessions: usize,
    pub max_sessions: usize,
    pub total_created: usize,
    pub total_expired: usize,
}

/// Thread-safe session manager
pub type SharedSessionManager = Arc<RwLock<SessionManager>>;

/// Create a shared session manager
pub fn create_session_manager(max_sessions: usize) -> SharedSessionManager {
    Arc::new(RwLock::new(SessionManager::new(max_sessions)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let session = Session::new(Some("user1".to_string()), SessionConfig::default());
        assert_eq!(session.state, SessionState::Active);
        assert_eq!(session.tokens_generated, 0);
    }

    #[test]
    fn test_session_lifecycle() {
        let mut session = Session::new(None, SessionConfig::default());
        assert_eq!(session.state, SessionState::Active);

        session.mark_idle();
        assert_eq!(session.state, SessionState::Idle);

        session.activate();
        assert_eq!(session.state, SessionState::Active);

        session.close();
        assert_eq!(session.state, SessionState::Closed);
    }

    #[test]
    fn test_prompt_caching() {
        let mut session = Session::new(None, SessionConfig::default());

        session.cache_prompt(12345, 100, 100);

        let pos = session.get_cached_prompt(12345);
        assert_eq!(pos, Some(100));

        let miss = session.get_cached_prompt(99999);
        assert_eq!(miss, None);
    }

    #[test]
    fn test_session_manager() {
        let mut manager = SessionManager::new(10);

        let session_id = manager
            .create_session(Some("user1".to_string()), None)
            .unwrap();
        assert!(manager.get_session(session_id).is_some());

        let same_session = manager.get_or_create_for_user("user1").unwrap();
        assert_eq!(session_id, same_session);

        assert!(manager.close_session(session_id));
    }
}
