use crate::models::{Backend, Cluster, StreamLoadResponse};
use crate::utils::{ApiError, ApiResult};
use serde_json::Value;

pub const MAX_STREAM_LOAD_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamLoadFormat {
    Csv,
    Json,
}

impl StreamLoadFormat {
    pub fn parse(value: &str) -> ApiResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "csv" => Ok(Self::Csv),
            "json" => Ok(Self::Json),
            _ => Err(ApiError::validation_error("Stream Load 仅支持 CSV 或 JSON 文件")),
        }
    }

    pub const fn as_header_value(self) -> Option<&'static str> {
        match self {
            Self::Csv => None,
            Self::Json => Some("json"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamLoadSpec {
    pub database: String,
    pub table: String,
    pub format: StreamLoadFormat,
    pub label: Option<String>,
    pub column_separator: Option<String>,
}

pub fn validate_stream_load_spec(
    database: &str,
    table: &str,
    format: &str,
    label: Option<&str>,
    column_separator: Option<&str>,
) -> ApiResult<StreamLoadSpec> {
    let database = validate_path_segment(database, "数据库")?;
    let table = validate_path_segment(table, "目标表")?;
    let format = StreamLoadFormat::parse(format)?;
    let label = label
        .map(|value| validate_header_value(value, "Label", 128))
        .transpose()?;
    let column_separator = match (format, column_separator.map(str::trim)) {
        (StreamLoadFormat::Csv, Some(separator @ ("," | "\\t" | "|"))) => {
            Some(separator.to_string())
        },
        (StreamLoadFormat::Csv, Some(_)) => {
            return Err(ApiError::validation_error("CSV 列分隔符仅支持逗号、制表符或竖线"));
        },
        (StreamLoadFormat::Csv, None) => Some(",".to_string()),
        (StreamLoadFormat::Json, _) => None,
    };

    Ok(StreamLoadSpec { database, table, format, label, column_separator })
}

pub fn stream_load_url(
    cluster: &Cluster,
    backend: &Backend,
    spec: &StreamLoadSpec,
) -> ApiResult<String> {
    let host = backend.host.trim();
    if host.is_empty() {
        return Err(ApiError::cluster_connection_failed("没有可用的 Stream Load 节点"));
    }
    let port = backend
        .http_port
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| ApiError::cluster_connection_failed("Stream Load 节点 HTTP 端口无效"))?;
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let protocol = if cluster.enable_ssl { "https" } else { "http" };
    Ok(format!(
        "{protocol}://{host}:{port}/api/{}/{}/_stream_load",
        urlencoding::encode(&spec.database),
        urlencoding::encode(&spec.table),
    ))
}

pub fn parse_stream_load_response(payload: &str) -> StreamLoadResponse {
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return StreamLoadResponse {
            success: false,
            status: None,
            label: None,
            message: Some("StarRocks 返回了无法解析的 Stream Load 结果".to_string()),
            number_total_rows: None,
            number_loaded_rows: None,
            number_filtered_rows: None,
            load_bytes: None,
        };
    };
    let status = value_string(&value, "Status");
    StreamLoadResponse {
        success: status
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case("success")),
        status,
        label: value_string(&value, "Label"),
        message: value_string(&value, "Message"),
        number_total_rows: value_u64(&value, "NumberTotalRows"),
        number_loaded_rows: value_u64(&value, "NumberLoadedRows"),
        number_filtered_rows: value_u64(&value, "NumberFilteredRows"),
        load_bytes: value_u64(&value, "LoadBytes"),
    }
}

fn validate_path_segment(value: &str, field: &str) -> ApiResult<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 256
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(ApiError::validation_error(format!("{field} 无效")));
    }
    Ok(value.to_string())
}

fn validate_header_value(value: &str, field: &str, max_len: usize) -> ApiResult<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_len || value.chars().any(char::is_control) {
        return Err(ApiError::validation_error(format!("{field} 无效")));
    }
    Ok(value.to_string())
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(|value| match value {
        Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn value_u64(value: &Value, key: &str) -> Option<u64> {
    value
        .get(key)
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
}
