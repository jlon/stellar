-- 事件保留裁剪索引：run_once 每 tick 按 last_seen_at 裁剪，无索引时全表扫描
CREATE INDEX idx_agent_events_last_seen ON agent_events(last_seen_at);
