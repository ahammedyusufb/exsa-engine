//! Multi-user stress test

#[cfg(test)]
mod tests {
    use exsa_engine::inference::{RequestQueue, SamplingParams};
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_concurrent_requests() {
        // Create queue
        let mut queue = RequestQueue::new(50);
        let handle = queue.handle();

        // Spawn multiple concurrent requests
        let mut handles = vec![];

        for i in 0..10 {
            let handle_clone = handle.clone();
            let task = tokio::spawn(async move {
                let prompt = format!("Test prompt {}", i);
                let params = SamplingParams::default();

                let result = handle_clone.submit(prompt, params).await;
                result.is_ok()
            });
            handles.push(task);
        }

        // Wait for all to complete
        let results = futures::future::join_all(handles).await;

        // Check all succeeded
        for result in results {
            assert!(result.is_ok(), "Task failed");
            assert!(result.unwrap(), "Request submission failed");
        }
    }

    #[tokio::test]
    async fn test_queue_capacity() {
        // Test queue behavior when full
        let queue_size = 5;
        let mut queue = RequestQueue::new(queue_size);
        let handle = queue.handle();

        // Fill the queue
        let mut requests = vec![];
        for i in 0..queue_size {
            let result = handle
                .submit(
                    format!("Prompt {}", i),
                    SamplingParams::default(),
                )
                .await;
            assert!(result.is_ok(), "Failed to submit request {}", i);
            requests.push(result.unwrap());
        }

        // Queue should still have capacity for more
        assert!(handle.capacity() > 0, "Queue has no capacity");
    }
}
