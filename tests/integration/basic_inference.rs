//! Basic inference integration test

#[cfg(test)]
mod tests {
    use exsa_engine::{
        inference::{InferenceEngine, RequestQueue, SamplingParams},
        model::ModelConfig,
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn test_request_queue_basic() {
        // Test basic queue functionality
        let queue_size = 10;
        let mut queue = RequestQueue::new(queue_size);
        let handle = queue.handle();

        // Submit a request
        let prompt = "Hello, world!".to_string();
        let params = SamplingParams::default();

        let result = handle.submit(prompt, params).await;
        assert!(result.is_ok(), "Failed to submit request to queue");
    }

    #[tokio::test]
    async fn test_sampling_params_validation() {
        // Test valid parameters
        let valid_params = SamplingParams {
            temperature: 0.7,
            top_k: 40,
            top_p: 0.95,
            repeat_penalty: 1.1,
            max_tokens: 100,
            ..Default::default()
        };
        assert!(valid_params.validate().is_ok());

        // Test invalid temperature
        let invalid_temp = SamplingParams {
            temperature: -1.0,
            ..Default::default()
        };
        assert!(invalid_temp.validate().is_err());

        // Test invalid top_p
        let invalid_top_p = SamplingParams {
            top_p: 1.5,
            ..Default::default()
        };
        assert!(invalid_top_p.validate().is_err());

        // Test invalid max_tokens
        let invalid_max = SamplingParams {
            max_tokens: 0,
            ..Default::default()
        };
        assert!(invalid_max.validate().is_err());
    }
}
