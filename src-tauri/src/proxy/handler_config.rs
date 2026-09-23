//! Handler 配置模块
//!
//! 定义各 API 处理器的配置结构和使用量解析器

use crate::app_config::AppType;
use crate::proxy::usage::parser::TokenUsage;
use serde_json::Value;

/// 使用量解析器类型别名
pub type StreamUsageParser = fn(&[Value]) -> Option<TokenUsage>;
pub type ResponseUsageParser = fn(&Value) -> Option<TokenUsage>;

/// 模型提取器类型别名
/// 参数: (流式事件列表, 请求中的模型名称) -> 最终使用的模型名称
pub type StreamModelExtractor = fn(&[Value], &str) -> String;

/// 流式 usage 事件预过滤器类型别名。
///
/// 参数是 SSE `data:` 原始字符串。返回 false 时跳过 JSON parse，避免在
/// token/chunk 高频路径上解析与 usage 无关的事件。
pub type StreamUsageEventFilter = fn(&str) -> bool;

/// 各 API 的使用量解析配置
#[derive(Clone, Copy)]
pub struct UsageParserConfig {
    /// 流式响应解析器
    pub stream_parser: StreamUsageParser,
    /// 非流式响应解析器
    pub response_parser: ResponseUsageParser,
    /// 流式响应中的模型提取器
    pub model_extractor: StreamModelExtractor,
    /// 流式 usage 事件预过滤器
    pub stream_event_filter: Option<StreamUsageEventFilter>,
    /// 流式「开始产出」事件预过滤器（首个 token 增量）
    ///
    /// 仅用于 first_token_ms（首字）计时。必须与 `stream_event_filter` 分开：
    /// usage 事件通常在流末尾（如 Codex 的 `response.completed`），用它计时会把
    /// 首字算成整段耗时。
    pub stream_start_filter: Option<StreamUsageEventFilter>,
    /// 应用类型字符串（用于日志记录）
    pub app_type_str: &'static str,
}

// ============================================================================
// 流式 usage 事件预过滤
// ============================================================================

pub fn claude_stream_usage_event_filter(data: &str) -> bool {
    data.contains("\"message_start\"") || data.contains("\"message_delta\"")
}

fn openai_stream_usage_event_filter(data: &str) -> bool {
    data.contains("\"usage\"")
}

pub fn codex_stream_usage_event_filter(data: &str) -> bool {
    data.contains("\"response.completed\"") || data.contains("\"usage\"")
}

fn gemini_stream_usage_event_filter(data: &str) -> bool {
    data.contains("\"usageMetadata\"")
}

// ============================================================================
// 流式「开始产出」事件预过滤（首字计时）
// ============================================================================
//
// 与上面的 usage 过滤器相互独立，只做子串匹配：命中即记录首字时间，
// 不解析 JSON，也不进入 usage 收集。

/// Claude Messages 首个产出事件（含 text / thinking / 工具入参增量）
pub fn claude_stream_start_filter(data: &str) -> bool {
    data.contains("\"content_block_delta\"")
}

/// OpenAI Chat Completions 首个产出事件（choices[].delta 增量）
fn openai_stream_start_filter(data: &str) -> bool {
    data.contains("\"delta\"")
}

/// Codex Responses 首个产出事件（各类 `.delta` 增量）
pub fn codex_stream_start_filter(data: &str) -> bool {
    data.contains("\"response.output_text.delta\"")
        || data.contains("\"response.reasoning_summary_text.delta\"")
        || data.contains("\"response.reasoning_text.delta\"")
        || data.contains("\"response.reasoning.delta\"")
        || data.contains("\"response.function_call_arguments.delta\"")
}

/// Gemini 首个产出事件（candidates[].content.parts[].text 增量）
fn gemini_stream_start_filter(data: &str) -> bool {
    data.contains("\"text\"")
}

// ============================================================================
// 模型提取器实现
// ============================================================================

/// Claude 流式响应模型提取（优先使用 usage.model）
///
/// 空字符串模型名视为缺失（转换层对无回显上游会合成 model:""），
/// 落到 fallback_model（映射后的出站模型或客户端请求模型）。
fn claude_model_extractor(events: &[Value], fallback_model: &str) -> String {
    // 首先尝试从解析的 usage 中获取模型
    if let Some(usage) = TokenUsage::from_claude_stream_events(events) {
        if let Some(model) = usage.model.filter(|m| !m.is_empty()) {
            return model;
        }
    }
    fallback_model.to_string()
}

/// OpenAI Chat Completions 流式响应模型提取（优先使用 usage.model）
fn openai_model_extractor(events: &[Value], fallback_model: &str) -> String {
    // 首先尝试从解析的 usage 中获取模型
    if let Some(usage) = TokenUsage::from_openai_stream_events(events) {
        if let Some(model) = usage.model.filter(|m| !m.is_empty()) {
            return model;
        }
    }
    // 回退：从事件中直接提取
    events
        .iter()
        .find_map(|e| e.get("model")?.as_str().filter(|m| !m.is_empty()))
        .unwrap_or(fallback_model)
        .to_string()
}

/// Codex 智能流式响应模型提取（自动检测格式）
fn codex_auto_model_extractor(events: &[Value], fallback_model: &str) -> String {
    // 首先尝试从解析的 usage 中获取模型
    if let Some(usage) = TokenUsage::from_codex_stream_events_auto(events) {
        if let Some(model) = usage.model.filter(|m| !m.is_empty()) {
            return model;
        }
    }
    // 回退：从 response.completed 事件中提取
    events
        .iter()
        .find_map(|e| {
            if e.get("type")?.as_str()? == "response.completed" {
                e.get("response")?
                    .get("model")?
                    .as_str()
                    .filter(|m| !m.is_empty())
            } else {
                None
            }
        })
        .or_else(|| {
            // 再回退：从 OpenAI 格式事件中提取
            events
                .iter()
                .find_map(|e| e.get("model")?.as_str().filter(|m| !m.is_empty()))
        })
        .unwrap_or(fallback_model)
        .to_string()
}

/// Gemini 流式响应模型提取（优先使用 usage.model）
fn gemini_model_extractor(events: &[Value], fallback_model: &str) -> String {
    // 首先尝试从解析的 usage 中获取模型
    if let Some(usage) = TokenUsage::from_gemini_stream_chunks(events) {
        if let Some(model) = usage.model.filter(|m| !m.is_empty()) {
            return model;
        }
    }
    fallback_model.to_string()
}

// ============================================================================
// 预定义配置
// ============================================================================

/// Claude API 解析配置
pub const CLAUDE_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_parser: TokenUsage::from_claude_stream_events,
    response_parser: TokenUsage::from_claude_response,
    model_extractor: claude_model_extractor,
    stream_event_filter: Some(claude_stream_usage_event_filter),
    stream_start_filter: Some(claude_stream_start_filter),
    app_type_str: "claude",
};

/// OpenAI Chat Completions API 解析配置（用于 Codex /v1/chat/completions）
pub const OPENAI_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_parser: TokenUsage::from_openai_stream_events,
    response_parser: TokenUsage::from_openai_response,
    model_extractor: openai_model_extractor,
    stream_event_filter: Some(openai_stream_usage_event_filter),
    stream_start_filter: Some(openai_stream_start_filter),
    app_type_str: "codex",
};

/// Codex 智能解析配置（自动检测 OpenAI 或 Codex 格式）
pub const CODEX_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_parser: TokenUsage::from_codex_stream_events_auto,
    response_parser: TokenUsage::from_codex_response_auto,
    model_extractor: codex_auto_model_extractor,
    stream_event_filter: Some(codex_stream_usage_event_filter),
    stream_start_filter: Some(codex_stream_start_filter),
    app_type_str: "codex",
};

/// Gemini API 解析配置
pub const GEMINI_PARSER_CONFIG: UsageParserConfig = UsageParserConfig {
    stream_parser: TokenUsage::from_gemini_stream_chunks,
    response_parser: TokenUsage::from_gemini_response,
    model_extractor: gemini_model_extractor,
    stream_event_filter: Some(gemini_stream_usage_event_filter),
    stream_start_filter: Some(gemini_stream_start_filter),
    app_type_str: "gemini",
};

// ============================================================================
// Handler 配置（预留，用于进一步简化）
// ============================================================================

/// Handler 基础配置
///
/// 预留结构，可用于进一步统一各 handler 的配置
#[allow(dead_code)]
#[derive(Clone)]
pub struct HandlerConfig {
    /// 应用类型
    pub app_type: AppType,
    /// 日志标签
    pub tag: &'static str,
    /// 应用类型字符串
    pub app_type_str: &'static str,
    /// 使用量解析配置
    pub parser_config: &'static UsageParserConfig,
}

/// Claude Handler 配置
#[allow(dead_code)]
pub const CLAUDE_HANDLER_CONFIG: HandlerConfig = HandlerConfig {
    app_type: AppType::Claude,
    tag: "Claude",
    app_type_str: "claude",
    parser_config: &CLAUDE_PARSER_CONFIG,
};

/// Codex Chat Completions Handler 配置
#[allow(dead_code)]
pub const CODEX_CHAT_HANDLER_CONFIG: HandlerConfig = HandlerConfig {
    app_type: AppType::Codex,
    tag: "Codex",
    app_type_str: "codex",
    parser_config: &OPENAI_PARSER_CONFIG,
};

/// Codex Responses Handler 配置
#[allow(dead_code)]
pub const CODEX_RESPONSES_HANDLER_CONFIG: HandlerConfig = HandlerConfig {
    app_type: AppType::Codex,
    tag: "Codex",
    app_type_str: "codex",
    parser_config: &CODEX_PARSER_CONFIG,
};

/// Gemini Handler 配置
#[allow(dead_code)]
pub const GEMINI_HANDLER_CONFIG: HandlerConfig = HandlerConfig {
    app_type: AppType::Gemini,
    tag: "Gemini",
    app_type_str: "gemini",
    parser_config: &GEMINI_PARSER_CONFIG,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_stream_start_filter_ignores_usage_only_events() {
        // codex 的 usage 只在 response.completed 出现（流末尾），不能参与首字计时
        assert!(codex_stream_start_filter(
            r#"{"type":"response.output_text.delta","delta":"hi"}"#
        ));
        assert!(codex_stream_start_filter(
            r#"{"type":"response.reasoning_summary_text.delta","delta":"think"}"#
        ));
        assert!(codex_stream_start_filter(
            r#"{"type":"response.function_call_arguments.delta","delta":"{}"}"#
        ));
        assert!(!codex_stream_start_filter(
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":1}}}"#
        ));
        assert!(!codex_stream_start_filter(r#"{"type":"response.created"}"#));
    }

    #[test]
    fn claude_and_gemini_stream_start_filter_ignore_leading_events() {
        assert!(claude_stream_start_filter(
            r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}"#
        ));
        assert!(!claude_stream_start_filter(r#"{"type":"message_start"}"#));
        assert!(!claude_stream_start_filter(r#"{"type":"message_delta"}"#));

        assert!(gemini_stream_start_filter(
            r#"{"candidates":[{"content":{"parts":[{"text":"hi"}]}}]}"#
        ));
        assert!(!gemini_stream_start_filter(
            r#"{"candidates":[{"finishReason":"STOP"}],"usageMetadata":{"totalTokenCount":3}}"#
        ));
    }
}
