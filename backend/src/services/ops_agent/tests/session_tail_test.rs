use sqlx::sqlite::SqlitePoolOptions;

use crate::services::ai::AiSessionStore;

async fn test_store() -> AiSessionStore<sqlx::Sqlite> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("connect test sqlite");
    sqlx::query(
        "CREATE TABLE ai_messages (\
            id INTEGER PRIMARY KEY, session_id INTEGER NOT NULL, role TEXT NOT NULL, \
            content TEXT NOT NULL, steps_json TEXT, context_json TEXT)",
    )
    .execute(&pool)
    .await
    .expect("create ai_messages");
    sqlx::query(
        "INSERT INTO ai_messages (id, session_id, role, content) VALUES \
         (1, 10, 'user', 'old'), (2, 20, 'user', 'other session'), (3, 10, 'assistant', 'tail')",
    )
    .execute(&pool)
    .await
    .expect("seed ai_messages");
    AiSessionStore::new(pool)
}

#[tokio::test]
async fn checkpoint_tail_query_is_session_scoped() {
    let store = test_store().await;
    let tail = store
        .load_chat_history_after(10, 1)
        .await
        .expect("load tail");
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].id, 3);

    let checkpoint = serde_json::json!({"kind": "ops_agent_compaction"});
    store
        .update_message_context(10, 3, &checkpoint)
        .await
        .expect("update own session message");
    assert_eq!(
        store.load_latest_context(10).await.unwrap().as_deref(),
        Some(r#"{"kind":"ops_agent_compaction"}"#)
    );
    assert!(
        store
            .update_message_context(20, 3, &checkpoint)
            .await
            .is_err()
    );
}
