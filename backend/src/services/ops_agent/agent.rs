//! The agent loop: stream-free implementation of the pi-from-scratch / Flink
//! assistant skeleton --
//! `chat -> tool_call -> execute -> append tool_result -> repeat until model stops`.
//! Every event is recorded as an `AgentStep` for the frontend invocation chain.

use serde_json::{Value, json};
use std::time::Instant;

use super::tool::{AgentTool, tool_specs};
use crate::services::ai::llm::ChatClient;
use crate::services::ai::types::{AgentStep, ChatMessage};

/// 流式进度事件：步骤（工具链）、模型文本增量（打字机）或回合语义标记。
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    Step(AgentStep),
    /// 当前回合模型内容增量。
    Delta(String),
    /// 回合结束语义标记：有工具调用 => Reasoning（增量归入思考段），
    /// 纯文本 => Answer（增量即最终答案）。前端据此把打字机缓冲归位。
    Phase(PhaseKind),
    /// 对话内动作申请（propose_action 返回 ACTION_PENDING 前缀时截获）。
    ActionRequest(serde_json::Value),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseKind {
    Reasoning,
    Answer,
}

/// Streaming progress sink: steps and content deltas are pushed as they happen.
pub type ProgressSink = tokio::sync::mpsc::UnboundedSender<ProgressEvent>;

/// Record a step both in the audit trace and (when streaming) on the sink.
fn record(steps: &mut Vec<AgentStep>, progress: &Option<ProgressSink>, step: AgentStep) {
    if let Some(tx) = progress {
        let _ = tx.send(ProgressEvent::Step(step.clone()));
    }
    steps.push(step);
}

/// Result of one agent turn.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentResult {
    pub steps: Vec<AgentStep>,
    pub final_answer: String,
    pub usage_tokens: i64,
}

/// Runs agent turns. Created per request; holds the LLM client and the tool registry.
pub struct OltpDiagnosisAgent {
    llm: ChatClient,
    tools: Vec<Box<dyn AgentTool>>,
    max_tool_calls: usize,
}

impl OltpDiagnosisAgent {
    pub fn new(llm: ChatClient, tools: Vec<Box<dyn AgentTool>>, max_tool_calls: usize) -> Self {
        Self { llm, tools, max_tool_calls: max_tool_calls.max(1) }
    }

    pub fn llm_available(&self) -> bool {
        self.llm.is_available()
    }

    /// Run one turn: system message + conversation history + user message.
    pub async fn run(
        &self,
        system: ChatMessage,
        history: Vec<ChatMessage>,
        user_message: &str,
    ) -> Result<AgentResult, String> {
        self.run_with_progress(system, history, user_message, None)
            .await
    }

    /// Streaming variant of `run`: pushes every step onto `progress` as it
    /// happens. When the client disconnects the sink's receive side drops and
    /// `send`/`is_closed` start failing; the loop then stops at the next round
    /// boundary instead of burning LLM calls (Flink lesson 9673029ef05 /
    /// 76594c573b8: cancellation must really release in-flight work).
    pub async fn run_with_progress(
        &self,
        system: ChatMessage,
        history: Vec<ChatMessage>,
        user_message: &str,
        progress: Option<ProgressSink>,
    ) -> Result<AgentResult, String> {
        if !self.llm.is_available() {
            return Err(
                "LLM provider 未配置或不可用（请在系统设置-LLM 配置中启用 Provider）".to_string()
            );
        }

        let mut messages = Vec::with_capacity(history.len() + 2);
        messages.push(system);
        messages.extend(history);
        messages.push(ChatMessage {
            role: "user".into(),
            content: user_message.to_string(),
            tool_calls: None,
            tool_call_id: None,
        });

        let mut steps = Vec::new();
        let mut usage_tokens: i64 = 0;
        let tools_spec = tool_specs(&self.tools);

        // 运行时工具护栏（借鉴 sxdevops/Ongrid 的"工具预算下沉后端"）：
        // 1) 同参重复调用拦截（模型可能重复查同一指标）；
        // 2) 连续空结果计数，达到阈值后提示模型停止空转。
        let mut call_history: Vec<(String, String)> = Vec::new(); // (name, args_hash)
        let mut empty_strikes: usize = 0;
        const MAX_EMPTY_STRIKES: usize = 2;
        // 连续失败计数：换参穷举同一失败工具会烧光轮次（实测 EXPLAIN 连撞 5 次撞上限）。
        let mut error_strikes: usize = 0;
        const MAX_ERROR_STRIKES: usize = 3;

        for round in 0..self.max_tool_calls {
            if let Some(tx) = &progress {
                if tx.is_closed() {
                    return Err("客户端已断开，本轮取消".to_string());
                }
            }
            // 流式 LLM 调用：内容增量经同步回调实时转发（打字机），工具调用 delta
            // 内部累积；回调只在读循环中同步执行，无独立转发任务。
            let completion = {
                let progress = &progress;
                self.llm
                    .chat_stream(&messages, Some(&tools_spec), |text| {
                        if let Some(tx) = progress {
                            let _ = tx.send(ProgressEvent::Delta(text));
                        }
                    })
                    .await?
            };
            usage_tokens += completion.usage_tokens;
            // 回合语义标记：有工具调用 => 思考段；纯文本 => 最终答案。
            // 前端据此把打字机缓冲归位（多工具轮不会把第二轮思考混入答案）。
            if let Some(tx) = &progress {
                let phase = if completion.tool_calls.is_empty() {
                    PhaseKind::Answer
                } else {
                    PhaseKind::Reasoning
                };
                let _ = tx.send(ProgressEvent::Phase(phase));
            }

            // 模型在工具调用回合穿插的思考文本（非空白）记录为推理步骤，
            // 对齐 Flink AgentStepKind.REASONING（DefaultJobDiagnosisAgent 的 thinking 段）。
            if !completion.tool_calls.is_empty() {
                if let Some(thinking) = completion
                    .content
                    .as_deref()
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                {
                    record(&mut steps, &progress, AgentStep::reasoning(thinking.to_string()));
                }
            }

            if completion.tool_calls.is_empty() {
                let answer = completion.content.unwrap_or_default().trim().to_string();
                record(&mut steps, &progress, AgentStep::end());
                return Ok(AgentResult { steps, final_answer: answer, usage_tokens });
            }

            // Execute all requested tool calls in order, appending results to context.
            let mut any_tool = false;
            for tc in &completion.tool_calls {
                if tc.name.is_empty() {
                    continue; // tolerate empty name deltas from some gateways
                }
                let start = Instant::now();
                let args: Value = serde_json::from_str(&tc.arguments).unwrap_or(json!({}));
                record(&mut steps, &progress, AgentStep::tool(&tc.name, args.clone()));

                // 同参重复调用拦截：相同工具+相同参数不重复执行，返回提示
                let args_hash = format!("{}:{:?}", tc.name, args);
                if call_history.iter().any(|(n, _)| n == &args_hash) {
                    let note = format!(
                        "工具 {} 已用相同参数查询过（结果未变化），无需重复调用，请基于已有证据继续。",
                        tc.name
                    );
                    record(
                        &mut steps,
                        &progress,
                        AgentStep::tool_result(&tc.name, note.clone(), 0),
                    );
                    messages.push(ChatMessage {
                        role: "tool".into(),
                        content: note,
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                    });
                    continue;
                }
                call_history.push((tc.name.clone(), args_hash));

                let result = self.execute_tool(&tc.name, &args).await;
                let duration_ms = start.elapsed().as_millis() as i64;
                let (result_text, tool_failed) = match &result {
                    Ok(text) => {
                        // 对话内动作申请：ACTION_PENDING 前缀 → 推确认卡事件
                        if let Some(stripped) = text.strip_prefix(
                            crate::services::ops_agent::chat_actions::ACTION_PENDING_PREFIX,
                        ) {
                            tracing::debug!("propose_action 截获，推送确认卡");
                            if let Ok(action) = serde_json::from_str::<serde_json::Value>(stripped)
                            {
                                if let Some(tx) = &progress {
                                    tracing::debug!(
                                        "propose_action 发送 ActionRequest (progress 存在)"
                                    );
                                    let _ = tx.send(ProgressEvent::ActionRequest(action));
                                } else {
                                    tracing::debug!(
                                        "propose_action 截获但 progress 为空（同步路径）"
                                    );
                                }
                            } else {
                                tracing::debug!("propose_action JSON 解析失败");
                            }
                        }
                        record(
                            &mut steps,
                            &progress,
                            AgentStep::tool_result(&tc.name, text.clone(), duration_ms),
                        );
                        (text.clone(), false)
                    },
                    Err(e) => {
                        record(&mut steps, &progress, AgentStep::error(&tc.name, e));
                        (e.clone(), true)
                    },
                };
                if tool_failed {
                    error_strikes += 1;
                    if error_strikes == MAX_ERROR_STRIKES {
                        let note = "工具已连续失败 3 次。请停止换参穷举：若失败原因是环境性的\
                         （无数据/功能未开启/库表不存在/超时），换一个工具或直接基于已有证据收敛结论。\
                         不要再用不同参数重复调用同一个失败的工具。".to_string();
                        record(
                            &mut steps,
                            &progress,
                            AgentStep::tool_result(&tc.name, note.clone(), 0),
                        );
                        messages.push(ChatMessage {
                            role: "tool".into(),
                            content: note,
                            tool_calls: None,
                            tool_call_id: Some(tc.id.clone()),
                        });
                        error_strikes = 0; // 提示一次即可，防噪音
                    }
                } else {
                    error_strikes = 0;
                }
                // 连续空结果计数：空/无数据结果达到阈值时给模型提示，避免空转
                let empty = result_text.trim().is_empty()
                    || result_text.contains("无数据")
                    || result_text.contains("暂无")
                    || result_text.contains("没有找到");
                if empty {
                    empty_strikes += 1;
                    if empty_strikes == MAX_EMPTY_STRIKES && !result_text.is_empty() {
                        let note = "检测到连续空/无数据结果。请停止空转：换一个工具或直接基于已有证据给出结论。".to_string();
                        record(
                            &mut steps,
                            &progress,
                            AgentStep::tool_result(&tc.name, note.clone(), 0),
                        );
                        messages.push(ChatMessage {
                            role: "tool".into(),
                            content: note,
                            tool_calls: None,
                            tool_call_id: Some(tc.id.clone()),
                        });
                        empty_strikes = 0; // 提示一次即可，防噪音
                    }
                } else {
                    empty_strikes = 0;
                }
                any_tool = true;
                messages.push(ChatMessage {
                    role: "assistant".into(),
                    content: String::new(),
                    // 序列化时自动转为 OpenAI 嵌套格式（ChatMessage 自定义 Serialize）
                    tool_calls: Some(vec![crate::services::ai::types::ToolCallDelta {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    }]),
                    tool_call_id: None,
                });
                messages.push(ChatMessage {
                    role: "tool".into(),
                    content: result_text,
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                });
            }

            if !any_tool {
                // Only empty tool calls came back; stop rather than looping forever.
                record(&mut steps, &progress, AgentStep::end());
                return Ok(AgentResult {
                    steps,
                    final_answer: "模型请求了工具但没有可用调用，请重试。".to_string(),
                    usage_tokens,
                });
            }

            // Keep round-boundary reasoning visible.
            record(
                &mut steps,
                &progress,
                AgentStep::reasoning(format!("工具执行完成（第 {} 轮），继续分析…", round + 1)),
            );
        }

        record(&mut steps, &progress, AgentStep::end());
        Ok(AgentResult {
            steps,
            final_answer: "已达到本轮工具调用上限，请基于已收集的证据缩小问题范围后重试。"
                .to_string(),
            usage_tokens,
        })
    }

    async fn execute_tool(&self, name: &str, args: &Value) -> Result<String, String> {
        for tool in &self.tools {
            if tool.name() == name {
                return tool.execute(args.clone()).await;
            }
        }
        Err(format!("未知工具: {}", name))
    }
}
