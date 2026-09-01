use std::fmt;
use std::io;
use std::io::Write;

use serde::de::DeserializeSeed;
use serde::de::Deserializer;
use serde::de::Error as _;
use serde::de::IgnoredAny;
use serde::de::MapAccess;
use serde::de::SeqAccess;
use serde::de::Visitor;
use serde_json::Value;

pub const MAX_JSON_BYTES: usize = 1_024 * 1_024;
pub const MAX_JSON_NODES: usize = 50_000;
pub const MAX_JSON_DEPTH: usize = 64;

pub fn parse_bounded_json(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > MAX_JSON_BYTES {
        return Err(format!("JSON exceeds {MAX_JSON_BYTES} bytes"));
    }
    let mut budget = JsonBudget { nodes: 0 };
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    JsonSeed {
        budget: &mut budget,
        depth: 0,
    }
    .deserialize(&mut deserializer)
    .map_err(|error| format!("invalid or over-complex JSON: {error}"))?;
    deserializer
        .end()
        .map_err(|error| format!("invalid JSON: {error}"))?;
    serde_json::from_slice(bytes).map_err(|error| format!("invalid JSON: {error}"))
}

pub fn encode_bounded_json(value: &Value, maximum_bytes: usize) -> Result<Vec<u8>, String> {
    validate_value(value)?;
    let mut output = CappedBuffer {
        bytes: Vec::with_capacity(maximum_bytes.min(8 * 1_024)),
        maximum_bytes,
    };
    serde_json::to_writer(&mut output, value).map_err(|error| error.to_string())?;
    Ok(output.bytes)
}

fn validate_value(value: &Value) -> Result<(), String> {
    let mut nodes = 1usize;
    let mut pending = vec![(value, 0usize)];
    while let Some((value, depth)) = pending.pop() {
        if depth > MAX_JSON_DEPTH {
            return Err("JSON nesting limit exceeded".to_string());
        }
        match value {
            Value::Array(values) => {
                schedule_children(values.iter(), depth, &mut nodes, &mut pending)?
            }
            Value::Object(values) => {
                schedule_children(values.values(), depth, &mut nodes, &mut pending)?
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                continue;
            }
        }
    }
    Ok(())
}

fn schedule_children<'a>(
    children: impl Iterator<Item = &'a Value>,
    depth: usize,
    nodes: &mut usize,
    pending: &mut Vec<(&'a Value, usize)>,
) -> Result<(), String> {
    for child in children {
        *nodes = (*nodes)
            .checked_add(1)
            .ok_or_else(|| "JSON node count overflowed".to_string())?;
        if *nodes > MAX_JSON_NODES {
            return Err("JSON node limit exceeded".to_string());
        }
        pending.push((child, depth + 1));
    }
    Ok(())
}

struct CappedBuffer {
    bytes: Vec<u8>,
    maximum_bytes: usize,
}

impl Write for CappedBuffer {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.len() > self.maximum_bytes.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("JSON output exceeds its byte limit"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct JsonBudget {
    nodes: usize,
}

struct JsonSeed<'a> {
    budget: &'a mut JsonBudget,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for JsonSeed<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        if self.depth > MAX_JSON_DEPTH {
            return Err(D::Error::custom("JSON nesting limit exceeded"));
        }
        self.budget.nodes = self
            .budget
            .nodes
            .checked_add(1)
            .ok_or_else(|| D::Error::custom("JSON node count overflowed"))?;
        if self.budget.nodes > MAX_JSON_NODES {
            return Err(D::Error::custom("JSON node limit exceeded"));
        }
        deserializer.deserialize_any(JsonVisitor {
            budget: self.budget,
            depth: self.depth,
        })
    }
}

struct JsonVisitor<'a> {
    budget: &'a mut JsonBudget,
    depth: usize,
}

impl<'de> Visitor<'de> for JsonVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded JSON")
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_borrowed_str<E>(self, _value: &'de str) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_string<E>(self, _value: String) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        JsonSeed {
            budget: self.budget,
            depth: self.depth + 1,
        }
        .deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence
            .next_element_seed(JsonSeed {
                budget: self.budget,
                depth: self.depth + 1,
            })?
            .is_some()
        {}
        Ok(())
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        while map.next_key::<IgnoredAny>()?.is_some() {
            map.next_value_seed(JsonSeed {
                budget: self.budget,
                depth: self.depth + 1,
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "bounded_json_tests.rs"]
mod tests;
