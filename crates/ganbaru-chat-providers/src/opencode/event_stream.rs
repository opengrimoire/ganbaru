//! Bounded OpenCode server-sent event decoding and response streaming.

use super::protocol::{MAX_EVENT_DATA_BYTES, protocol_error};
use crate::models::ChatResult;
use serde_json::Value;
use std::collections::VecDeque;

const MAX_EVENT_FRAMING_BYTES: usize = 64 * 1024;

pub struct OpenCodeEventStream {
    response: reqwest::Response,
    decoder: OpenCodeSseDecoder,
    pending: VecDeque<Value>,
}

impl OpenCodeEventStream {
    pub fn new(response: reqwest::Response) -> Self {
        Self {
            response,
            decoder: OpenCodeSseDecoder::default(),
            pending: VecDeque::new(),
        }
    }

    pub async fn next(&mut self) -> ChatResult<Option<Value>> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(Some(event));
            }
            match self.response.chunk().await {
                Ok(Some(chunk)) => {
                    self.pending.extend(self.decoder.feed(&chunk)?);
                }
                Ok(None) => {
                    self.pending.extend(self.decoder.finish()?);
                    return Ok(self.pending.pop_front());
                }
                Err(_) => return Err(protocol_error("event stream transport")),
            }
        }
    }
}

#[derive(Default)]
pub struct OpenCodeSseDecoder {
    line_buffer: Vec<u8>,
    data_lines: Vec<Vec<u8>>,
    data_bytes: usize,
}

impl OpenCodeSseDecoder {
    pub fn feed(&mut self, chunk: &[u8]) -> ChatResult<Vec<Value>> {
        if chunk.len()
            > MAX_EVENT_DATA_BYTES
                .saturating_add(MAX_EVENT_FRAMING_BYTES)
                .saturating_sub(self.line_buffer.len())
        {
            return Err(protocol_error("event stream size"));
        }
        self.line_buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(newline) = self.line_buffer.iter().position(|byte| *byte == b'\n') {
            let mut line = self.line_buffer.drain(..=newline).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some(event) = self.accept_line(&line)? {
                events.push(event);
            }
        }
        if self.line_buffer.len() > MAX_EVENT_DATA_BYTES + MAX_EVENT_FRAMING_BYTES {
            return Err(protocol_error("event stream line size"));
        }
        Ok(events)
    }

    pub fn finish(&mut self) -> ChatResult<Vec<Value>> {
        let mut events = Vec::new();
        if !self.line_buffer.is_empty() {
            let mut line = std::mem::take(&mut self.line_buffer);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some(event) = self.accept_line(&line)? {
                events.push(event);
            }
        }
        if let Some(event) = self.dispatch()? {
            events.push(event);
        }
        Ok(events)
    }

    fn accept_line(&mut self, line: &[u8]) -> ChatResult<Option<Value>> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(b":") {
            return Ok(None);
        }
        let Some(colon_index) = line.iter().position(|byte| *byte == b':') else {
            return Ok(None);
        };
        let field = &line[..colon_index];
        let mut value = &line[colon_index + 1..];
        if field != b"data" {
            return Ok(None);
        }
        if value.first() == Some(&b' ') {
            value = &value[1..];
        }
        let separator_bytes = usize::from(!self.data_lines.is_empty());
        if value.len() + separator_bytes > MAX_EVENT_DATA_BYTES.saturating_sub(self.data_bytes) {
            return Err(protocol_error("event data size"));
        }
        self.data_bytes += value.len() + separator_bytes;
        self.data_lines.push(value.to_vec());
        Ok(None)
    }

    fn dispatch(&mut self) -> ChatResult<Option<Value>> {
        if self.data_lines.is_empty() {
            return Ok(None);
        }
        let data_lines = std::mem::take(&mut self.data_lines);
        self.data_bytes = 0;
        let mut data = Vec::new();
        for (index, line) in data_lines.into_iter().enumerate() {
            if index > 0 {
                data.push(b'\n');
            }
            data.extend(line);
        }
        serde_json::from_slice(&data)
            .map(Some)
            .map_err(|_| protocol_error("event data"))
    }
}
