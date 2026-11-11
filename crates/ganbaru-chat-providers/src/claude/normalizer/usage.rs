//! Claude token usage and context fill.

use super::{SYNTHETIC_MODEL, bounded_text, text, unsigned};
use crate::events::*;
use serde_json::{Map, Value};

/// Byte limit of a model id remembered for matching `modelUsage` entries.
const MAX_MODEL_ID_BYTES: usize = 256;

/// Token counts Claude reports for one API request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ClaudeRequestTokens {
    input: Option<u64>,
    cache_creation: Option<u64>,
    cache_read: Option<u64>,
    output: Option<u64>,
}

impl ClaudeRequestTokens {
    fn read(usage: &Map<String, Value>) -> Self {
        Self {
            input: unsigned(usage, "input_tokens"),
            cache_creation: unsigned(usage, "cache_creation_input_tokens"),
            cache_read: unsigned(usage, "cache_read_input_tokens"),
            output: unsigned(usage, "output_tokens"),
        }
    }

    /// Applies `message_delta` usage, which is cumulative for the message: absent or null fields
    /// keep the `message_start` values and reported values, zeros included, replace them.
    fn merge(&mut self, delta: Self) {
        self.input = delta.input.or(self.input);
        self.cache_creation = delta.cache_creation.or(self.cache_creation);
        self.cache_read = delta.cache_read.or(self.cache_read);
        self.output = delta.output.or(self.output);
    }

    /// Every input token the request sent, cached or not.
    fn prompt(&self) -> u64 {
        [self.input, self.cache_creation, self.cache_read]
            .into_iter()
            .flatten()
            .fold(0, u64::saturating_add)
    }

    /// The context window the request filled: its prompt plus the tokens it generated.
    fn context(&self) -> u64 {
        self.prompt().saturating_add(self.output.unwrap_or(0))
    }
}

/// The newest root API request of the turn.
#[derive(Clone, Debug)]
struct ClaudeRequestUsage {
    message_id: Option<String>,
    tokens: ClaudeRequestTokens,
}

/// Usage Claude reported for root model requests, read when the turn's `result` arrives.
///
/// Subagent requests run in their own context and `<synthetic>` notices report zero usage, so
/// neither moves the context fill.
#[derive(Clone, Debug, Default)]
pub(in crate::claude) struct ClaudeUsageTracker {
    latest: Option<ClaudeRequestUsage>,
    main_model: Option<String>,
}

impl ClaudeUsageTracker {
    /// Records the model a session was initialized with, unless a root request already named one.
    pub(super) fn session_model(&mut self, model: Option<&str>) {
        if self.main_model.is_none() {
            self.remember_model(model);
        }
    }

    /// Starts a root request from a `message_start` message.
    pub(super) fn message_start(&mut self, message: &Map<String, Value>, message_id: Option<&str>) {
        self.remember_model(text(message, "model"));
        let tokens = message
            .get("usage")
            .and_then(Value::as_object)
            .map(ClaudeRequestTokens::read)
            .unwrap_or_default();
        self.latest = Some(ClaudeRequestUsage {
            message_id: message_id.map(str::to_string),
            tokens,
        });
    }

    /// Merges the `message_delta` usage of the root request being streamed.
    pub(super) fn message_delta(&mut self, event: &Map<String, Value>) {
        let (Some(latest), Some(usage)) = (
            self.latest.as_mut(),
            event.get("usage").and_then(Value::as_object),
        ) else {
            return;
        };
        latest.tokens.merge(ClaudeRequestTokens::read(usage));
    }

    /// Reads the usage of a root assistant message, the only report of a request that was not
    /// streamed and a fallback for a streamed one whose stream reported no input.
    pub(super) fn assistant(&mut self, message: &Map<String, Value>, message_id: Option<&str>) {
        let model = text(message, "model");
        if model == Some(SYNTHETIC_MODEL) {
            return;
        }
        self.remember_model(model);
        let Some(usage) = message.get("usage").and_then(Value::as_object) else {
            return;
        };
        let reported = ClaudeRequestTokens::read(usage);
        match self.latest.as_mut() {
            Some(latest) if message_id.is_some() && latest.message_id.as_deref() == message_id => {
                if latest.tokens.prompt() == 0 && reported.prompt() > 0 {
                    let output = latest.tokens.output.max(reported.output);
                    latest.tokens = ClaudeRequestTokens { output, ..reported };
                }
            }
            _ => {
                self.latest = Some(ClaudeRequestUsage {
                    message_id: message_id.map(str::to_string),
                    tokens: reported,
                });
            }
        }
    }

    /// Builds the turn's usage from `result` and forgets the turn's requests.
    ///
    /// Counts are the turn totals Claude sums in `result.usage`; context fill is the newest root
    /// request against the window of the main model, which is the model root requests and the
    /// session init named, else `route_model`.
    pub(super) fn finish_turn(
        &mut self,
        object: &Map<String, Value>,
        route_model: Option<&str>,
    ) -> Option<ThreadUsageUpdatedEvent> {
        let latest = self.latest.take();
        let usage = object.get("usage")?.as_object()?;
        let cost = object
            .get("total_cost_usd")
            .and_then(Value::as_f64)
            .filter(|amount| amount.is_finite())
            .map(|amount| ProviderAttributedCost {
                amount,
                currency: "USD".to_string(),
                provider_reported: true,
            });
        Some(ThreadUsageUpdatedEvent {
            input_tokens: unsigned(usage, "input_tokens"),
            output_tokens: unsigned(usage, "output_tokens"),
            cached_input_tokens: unsigned(usage, "cache_read_input_tokens"),
            cache_write_input_tokens: unsigned(usage, "cache_creation_input_tokens"),
            context_tokens: latest
                .map(|request| request.tokens.context())
                .filter(|tokens| *tokens > 0),
            context_limit: self.context_limit(object, route_model),
            cost,
        })
    }

    /// The `contextWindow` of the main model's `modelUsage` entry, or the largest window when the
    /// main model has no entry.
    fn context_limit(&self, object: &Map<String, Value>, route_model: Option<&str>) -> Option<u64> {
        let models = object.get("modelUsage")?.as_object()?;
        let window = |entry: &Value| entry.get("contextWindow").and_then(Value::as_u64);
        self.main_model
            .as_deref()
            .or(route_model)
            .and_then(|model| models.get(model))
            .and_then(window)
            .or_else(|| models.values().filter_map(window).max())
    }

    fn remember_model(&mut self, model: Option<&str>) {
        if let Some(model) = model.filter(|model| !model.is_empty() && *model != SYNTHETIC_MODEL) {
            self.main_model = Some(bounded_text(model, MAX_MODEL_ID_BYTES));
        }
    }
}
