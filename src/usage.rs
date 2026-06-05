use serde_json::Value;
use std::fs;
use std::path::Path;

const TOKENS_PER_MILLION: f64 = 1_000_000.0;
const COST_KEYS: &[&str] = &[
    "cost_usd",
    "costUSD",
    "total_cost",
    "totalCost",
    "total_cost_usd",
    "totalCostUSD",
];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct UsageStats {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_creation_5m_tokens: u64,
    pub cache_creation_1h_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_usd: Option<f64>,
    pub model: String,
}

impl UsageStats {
    pub fn cached_tokens(&self) -> u64 {
        self.cache_creation_tokens + self.cache_read_tokens
    }

    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cached_tokens()
    }

    pub fn is_empty(&self) -> bool {
        self.total_tokens() == 0 && self.cost_usd.unwrap_or(0.0) == 0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_write_5m_per_mtok: f64,
    pub cache_write_1h_per_mtok: f64,
    pub cache_read_per_mtok: f64,
}

impl ModelPricing {
    const fn new(
        input_per_mtok: f64,
        output_per_mtok: f64,
        cache_write_5m_per_mtok: f64,
        cache_write_1h_per_mtok: f64,
        cache_read_per_mtok: f64,
    ) -> Self {
        Self {
            input_per_mtok,
            output_per_mtok,
            cache_write_5m_per_mtok,
            cache_write_1h_per_mtok,
            cache_read_per_mtok,
        }
    }
}

#[derive(Debug, Default)]
struct UsageAccumulator {
    stats: UsageStats,
    explicit_cost_usd: f64,
    explicit_cost_seen: bool,
    estimated_cost_usd: f64,
    estimated_cost_seen: bool,
}

impl UsageAccumulator {
    fn add_event(&mut self, value: &Value) {
        let Some(usage) = usage_value(value) else {
            return;
        };

        let event_model = model_name(value)
            .map(ToString::to_string)
            .or_else(|| (!self.stats.model.is_empty()).then(|| self.stats.model.clone()))
            .unwrap_or_default();

        if !event_model.is_empty() {
            self.stats.model = event_model.clone();
        }

        let mut event_stats = UsageStats {
            input_tokens: token_field(usage, "input_tokens"),
            output_tokens: token_field(usage, "output_tokens"),
            cache_read_tokens: token_field(usage, "cache_read_input_tokens"),
            model: event_model,
            ..UsageStats::default()
        };

        let cache_creation = token_field(usage, "cache_creation_input_tokens");
        event_stats.cache_creation_tokens = cache_creation;

        let cache_creation_detail = usage.get("cache_creation");
        let cache_creation_5m = cache_creation_detail
            .map(|detail| token_field(detail, "ephemeral_5m_input_tokens"))
            .unwrap_or(0);
        let cache_creation_1h = cache_creation_detail
            .map(|detail| token_field(detail, "ephemeral_1h_input_tokens"))
            .unwrap_or(0);

        if cache_creation_5m > 0 || cache_creation_1h > 0 {
            event_stats.cache_creation_5m_tokens = cache_creation_5m;
            event_stats.cache_creation_1h_tokens = cache_creation_1h;
        } else {
            event_stats.cache_creation_5m_tokens = cache_creation;
        }

        self.stats.input_tokens += event_stats.input_tokens;
        self.stats.output_tokens += event_stats.output_tokens;
        self.stats.cache_read_tokens += event_stats.cache_read_tokens;
        self.stats.cache_creation_tokens += event_stats.cache_creation_tokens;
        self.stats.cache_creation_5m_tokens += event_stats.cache_creation_5m_tokens;
        self.stats.cache_creation_1h_tokens += event_stats.cache_creation_1h_tokens;

        if let Some(cost) = cost_usd(value)
            .or_else(|| value.get("message").and_then(cost_usd))
            .or_else(|| cost_usd(usage))
        {
            self.explicit_cost_usd += cost;
            self.explicit_cost_seen = true;
        } else if let Some(cost) = estimate_cost_usd(&event_stats) {
            self.estimated_cost_usd += cost;
            self.estimated_cost_seen = true;
        }
    }

    fn finish(mut self) -> Option<UsageStats> {
        if self.explicit_cost_seen {
            self.stats.cost_usd = Some(self.explicit_cost_usd);
        } else if self.estimated_cost_seen {
            self.stats.cost_usd = Some(self.estimated_cost_usd);
        } else {
            self.stats.cost_usd = estimate_cost_usd(&self.stats);
        }

        (!self.stats.is_empty()).then_some(self.stats)
    }
}

pub fn extract_usage(input: &Value) -> Option<UsageStats> {
    if let Some(path) = input
        .get("transcript_path")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        && let Ok(Some(stats)) = summarize_transcript(path)
    {
        return Some(stats);
    }

    let mut acc = UsageAccumulator::default();
    acc.add_event(input);
    acc.finish()
}

pub fn summarize_transcript(path: impl AsRef<Path>) -> std::io::Result<Option<UsageStats>> {
    let content = fs::read_to_string(path)?;
    Ok(summarize_transcript_str(&content))
}

pub fn summarize_transcript_str(content: &str) -> Option<UsageStats> {
    let mut acc = UsageAccumulator::default();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        acc.add_event(&value);
    }

    acc.finish()
}

pub fn estimate_cost_usd(stats: &UsageStats) -> Option<f64> {
    let pricing = pricing_for_model(&stats.model)?;
    let categorized_cache_creation =
        stats.cache_creation_5m_tokens + stats.cache_creation_1h_tokens;
    let uncategorized_cache_creation = stats
        .cache_creation_tokens
        .saturating_sub(categorized_cache_creation);
    let cache_creation_5m = stats.cache_creation_5m_tokens + uncategorized_cache_creation;

    let cost = (stats.input_tokens as f64 * pricing.input_per_mtok
        + stats.output_tokens as f64 * pricing.output_per_mtok
        + cache_creation_5m as f64 * pricing.cache_write_5m_per_mtok
        + stats.cache_creation_1h_tokens as f64 * pricing.cache_write_1h_per_mtok
        + stats.cache_read_tokens as f64 * pricing.cache_read_per_mtok)
        / TOKENS_PER_MILLION;

    (cost > 0.0).then_some(cost)
}

pub fn pricing_for_model(model: &str) -> Option<ModelPricing> {
    let model = model.to_ascii_lowercase();

    if model_contains_any(&model, &["opus-4-7", "opus-4-6", "opus-4-5"]) {
        Some(ModelPricing::new(5.0, 25.0, 6.25, 10.0, 0.50))
    } else if model_contains_any(&model, &["opus-4-1", "opus-4", "opus-3"]) {
        Some(ModelPricing::new(15.0, 75.0, 18.75, 30.0, 1.50))
    } else if model_contains_any(
        &model,
        &[
            "sonnet-4-6",
            "sonnet-4-5",
            "sonnet-4",
            "sonnet-3-7",
            "sonnet-3.7",
            "sonnet-3-5",
            "sonnet-3.5",
        ],
    ) {
        Some(ModelPricing::new(3.0, 15.0, 3.75, 6.0, 0.30))
    } else if model_contains_any(&model, &["haiku-4-5", "haiku-4.5"]) {
        Some(ModelPricing::new(1.0, 5.0, 1.25, 2.0, 0.10))
    } else if model_contains_any(&model, &["haiku-3-5", "haiku-3.5"]) {
        Some(ModelPricing::new(0.80, 4.0, 1.0, 1.6, 0.08))
    } else if model_contains_any(&model, &["haiku-3"]) {
        Some(ModelPricing::new(0.25, 1.25, 0.30, 0.50, 0.03))
    } else {
        None
    }
}

pub fn compact_model_name(model: &str) -> String {
    let model = model.strip_prefix("claude-").unwrap_or(model);
    let parts: Vec<&str> = model.split('-').collect();

    if parts.len() >= 3
        && matches!(parts[0], "opus" | "sonnet" | "haiku")
        && parts[1].chars().all(|ch| ch.is_ascii_digit())
        && parts[2].chars().all(|ch| ch.is_ascii_digit())
    {
        return format!("{}-{}.{}", parts[0], parts[1], parts[2]);
    }

    model.to_string()
}

fn usage_value(value: &Value) -> Option<&Value> {
    value
        .get("message")
        .and_then(|message| message.get("usage"))
        .or_else(|| value.get("usage"))
}

fn model_name(value: &Value) -> Option<&str> {
    value
        .get("message")
        .and_then(|message| message.get("model"))
        .and_then(Value::as_str)
        .or_else(|| value.get("model").and_then(Value::as_str))
        .filter(|model| !model.is_empty())
}

fn token_field(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(value_as_u64).unwrap_or(0)
}

fn value_as_u64(value: &Value) -> Option<u64> {
    if let Some(value) = value.as_u64() {
        return Some(value);
    }

    if let Some(value) = value.as_i64().filter(|value| *value >= 0) {
        return Some(value as u64);
    }

    value
        .as_str()
        .and_then(|value| value.trim().parse::<u64>().ok())
}

fn cost_usd(value: &Value) -> Option<f64> {
    let object = value.as_object()?;
    for key in COST_KEYS {
        if let Some(cost) = object.get(*key).and_then(value_as_f64) {
            return Some(cost);
        }
    }
    None
}

fn value_as_f64(value: &Value) -> Option<f64> {
    let cost = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))?;
    (cost.is_finite() && cost >= 0.0).then_some(cost)
}

fn model_contains_any(model: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| model.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn test_extract_usage_from_message_usage() {
        let input = json!({
            "message": {
                "model": "claude-sonnet-4-6-20260101",
                "usage": {
                    "input_tokens": 1000,
                    "output_tokens": 200,
                    "cache_creation_input_tokens": 300,
                    "cache_read_input_tokens": 400
                }
            }
        });

        let stats = extract_usage(&input).unwrap();

        assert_eq!(stats.input_tokens, 1000);
        assert_eq!(stats.output_tokens, 200);
        assert_eq!(stats.cache_creation_tokens, 300);
        assert_eq!(stats.cache_read_tokens, 400);
        assert_eq!(stats.total_tokens(), 1900);
        assert_eq!(stats.model, "claude-sonnet-4-6-20260101");
        assert!(stats.cost_usd.is_some());
    }

    #[test]
    fn test_summarize_transcript_aggregates_jsonl_usage() {
        let content = r#"
{"type":"assistant","message":{"model":"claude-haiku-4-5-20251001","usage":{"input_tokens":3,"cache_creation_input_tokens":37100,"cache_read_input_tokens":0,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":37100},"output_tokens":3}}}
{"type":"assistant","message":{"model":"claude-haiku-4-5-20251001","usage":{"input_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":100,"output_tokens":20}}}
"#;

        let stats = summarize_transcript_str(content).unwrap();

        assert_eq!(stats.input_tokens, 13);
        assert_eq!(stats.output_tokens, 23);
        assert_eq!(stats.cache_creation_tokens, 37100);
        assert_eq!(stats.cache_creation_1h_tokens, 37100);
        assert_eq!(stats.cache_read_tokens, 100);
        assert_eq!(stats.total_tokens(), 37236);
        assert_eq!(stats.model, "claude-haiku-4-5-20251001");
    }

    #[test]
    fn test_extract_usage_prefers_transcript_path() {
        let path = std::env::temp_dir().join(format!(
            "wezterm-agent-dashboard-usage-{}.jsonl",
            std::process::id()
        ));
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            "{}",
            json!({
                "message": {
                    "model": "claude-sonnet-4-5-20250929",
                    "usage": {"input_tokens": 2, "output_tokens": 3}
                }
            })
        )
        .unwrap();
        drop(file);

        let input = json!({
            "transcript_path": path.to_string_lossy(),
            "message": {
                "model": "claude-sonnet-4-5-20250929",
                "usage": {"input_tokens": 999, "output_tokens": 999}
            }
        });

        let stats = extract_usage(&input).unwrap();
        assert_eq!(stats.input_tokens, 2);
        assert_eq!(stats.output_tokens, 3);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_explicit_cost_is_used() {
        let input = json!({
            "model": "unknown-model",
            "usage": {"input_tokens": 100, "output_tokens": 50},
            "cost_usd": "0.0123"
        });

        let stats = extract_usage(&input).unwrap();

        assert_eq!(stats.cost_usd, Some(0.0123));
    }

    #[test]
    fn test_unknown_model_has_no_estimated_cost() {
        let input = json!({
            "model": "unknown-model",
            "usage": {"input_tokens": 100, "output_tokens": 50}
        });

        let stats = extract_usage(&input).unwrap();

        assert_eq!(stats.cost_usd, None);
    }

    #[test]
    fn test_estimate_cost_uses_cache_duration() {
        let stats = UsageStats {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_creation_tokens: 2_000_000,
            cache_creation_5m_tokens: 1_000_000,
            cache_creation_1h_tokens: 1_000_000,
            cache_read_tokens: 1_000_000,
            cost_usd: None,
            model: "claude-sonnet-4-6".into(),
        };

        let cost = estimate_cost_usd(&stats).unwrap();

        assert!((cost - 28.05).abs() < f64::EPSILON);
    }

    #[test]
    fn test_transcript_estimates_cost_per_message_model() {
        let content = r#"
{"message":{"model":"claude-haiku-4-5-20251001","usage":{"input_tokens":1000000,"output_tokens":0}}}
{"message":{"model":"claude-sonnet-4-6-20260101","usage":{"input_tokens":1000000,"output_tokens":0}}}
"#;

        let stats = summarize_transcript_str(content).unwrap();

        assert_eq!(stats.input_tokens, 2_000_000);
        assert_eq!(stats.cost_usd, Some(4.0));
    }

    #[test]
    fn test_pricing_for_current_claude_families() {
        assert_eq!(
            pricing_for_model("claude-opus-4-6").unwrap().input_per_mtok,
            5.0
        );
        assert_eq!(
            pricing_for_model("claude-haiku-4-5-20251001")
                .unwrap()
                .output_per_mtok,
            5.0
        );
        assert!(pricing_for_model("gpt-unknown").is_none());
    }

    #[test]
    fn test_compact_model_name() {
        assert_eq!(
            compact_model_name("claude-sonnet-4-6-20260101"),
            "sonnet-4.6"
        );
        assert_eq!(compact_model_name("gpt-5.1-codex"), "gpt-5.1-codex");
    }
}
