use std::time::Duration;

use chrono::Utc;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::services::ai::{ChatClient, ChatMessage};
use crate::services::llm::{LLMProvider, ResolvedLLMProvider};

fn test_provider(api_base: String) -> ResolvedLLMProvider {
    let now = Utc::now();
    ResolvedLLMProvider::new(
        LLMProvider {
            id: 1,
            name: "test".into(),
            display_name: "Test".into(),
            api_base,
            model_name: "test-model".into(),
            api_key_encrypted: None,
            is_active: true,
            max_tokens: 64,
            temperature: 0.0,
            timeout_seconds: 30,
            enabled: true,
            priority: 1,
            created_at: now,
            updated_at: now,
        },
        "test-key".into(),
    )
}

#[tokio::test]
async fn stream_request_stops_while_waiting_for_response_headers() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock LLM");
    let address = listener.local_addr().expect("mock LLM address");
    let (request_seen_tx, request_seen) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept LLM request");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).await.expect("read LLM request");
            if count == 0 {
                return;
            }
            request.extend_from_slice(&buffer[..count]);
        }
        let _ = request_seen_tx.send(());
        std::future::pending::<()>().await;
    });

    let cancellation = CancellationToken::new();
    let request_cancellation = cancellation.clone();
    let client = ChatClient::new(test_provider(format!("http://{address}")));
    let request = tokio::spawn(async move {
        client
            .chat_stream(
                &[ChatMessage {
                    role: "user".into(),
                    content: "test".into(),
                    tool_calls: None,
                    tool_call_id: None,
                }],
                None,
                Some(request_cancellation),
                |_| {},
            )
            .await
    });

    tokio::time::timeout(Duration::from_secs(1), request_seen)
        .await
        .expect("request should reach the mock LLM")
        .expect("mock LLM should report the request");
    cancellation.cancel();

    let result = tokio::time::timeout(Duration::from_secs(1), request)
        .await
        .expect("cancellation should not wait for the request timeout")
        .expect("request task should not panic");
    assert_eq!(result.expect_err("cancelled request should fail"), "AI 请求已取消");
    server.abort();
}
