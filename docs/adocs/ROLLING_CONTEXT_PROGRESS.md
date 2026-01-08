# Rolling Context Window - Implementation Plan

## Status: IN PROGRESS

### Step 1: Core Context Manager ✅
- Created `src/inference/context.rs`
- Implements `ContextWindowManager` with:
  - Message history tracking
  - Automatic window shifting
  - System prompt preservation  
  - Token counting
  - Usage statistics

### Step 2: Integration (IN PROGRESS)
Need to integrate with inference engine:

1. **Add to InferenceEngine**:
   - Add `context_manager: Option<ContextWindowManager>` field
   - Initialize with context size from config
   
2. **Add to API**:
   - Track conversation history per session
   - Use context manager for multi-turn chats
   
3. **Wire to Chat Endpoint**:
   - Accumulate messages in context
   - Check space before generation
   - Shift window if needed

### Step 3: Testing
- Test with long conversations
- Verify system prompt preservation
- Confirm no crashes at context limit

### Next Actions
1. Integrate context manager into engine
2. Add session management for chat API
3. Test with multi-turn conversations

---

**Current**: Context manager implemented, ready for integration
