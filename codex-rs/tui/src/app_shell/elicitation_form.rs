use serde_json::Map;
use serde_json::Value;
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ElicitationForm {
    fields: Vec<Field>,
    index: usize,
    content: Map<String, Value>,
}
pub(crate) struct ElicitationFieldView<'a> {
    pub(crate) position: usize,
    pub(crate) total: usize,
    pub(crate) label: &'a str,
    pub(crate) required: bool,
    pub(crate) detail: String,
    pub(crate) input_hint: String,
}
#[derive(Debug, Clone, PartialEq)]
struct Field {
    name: String,
    label: String,
    description: Option<String>,
    required: bool,
    default: Option<Value>,
    kind: FieldKind,
}
#[derive(Debug, Clone, PartialEq)]
enum FieldKind {
    Text,
    Number(bool),
    Boolean,
    Choice(Vec<OptionItem>, bool),
    Json,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct OptionItem {
    value: String,
    label: String,
}
impl ElicitationForm {
    pub(super) fn from_schema(schema: &Value) -> Self {
        let required = schema.get("required").and_then(Value::as_array);
        let fields = schema
            .get("properties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(name, schema)| {
                let required = required.is_some_and(|required| {
                    required.iter().any(|value| value.as_str() == Some(name))
                });
                Field::new(name, schema, required)
            })
            .collect();
        Self {
            fields,
            index: 0,
            content: Map::new(),
        }
    }
    pub(super) fn complete(&self) -> bool {
        self.index >= self.fields.len()
    }
    pub(super) fn action_label(&self) -> &'static str {
        match self.fields.len().saturating_sub(self.index) {
            0 => "Accept",
            1 => "Submit",
            _ => "Next",
        }
    }
    pub(super) fn field_view(&self) -> Option<ElicitationFieldView<'_>> {
        let field = self.fields.get(self.index)?;
        let default_detail = if field.required {
            "Required"
        } else {
            "Optional"
        };
        Some(ElicitationFieldView {
            position: self.index.saturating_add(1),
            total: self.fields.len(),
            label: &field.label,
            required: field.required,
            detail: field
                .description
                .clone()
                .unwrap_or_else(|| default_detail.to_string()),
            input_hint: field.kind.hint(),
        })
    }
    pub(super) fn default_input(&self) -> Option<String> {
        self.fields.get(self.index)?.default_input()
    }
    pub(super) fn answer(&mut self, answer: &str) -> Result<(), String> {
        let field = self
            .fields
            .get(self.index)
            .cloned()
            .ok_or_else(|| "MCP form has no current field".to_string())?;
        if let Some(value) = field.parse(answer)? {
            self.content.insert(field.name, value);
        }
        self.index = self.index.saturating_add(1);
        Ok(())
    }
    pub(super) fn content(&self) -> Value {
        Value::Object(self.content.clone())
    }
}
impl Field {
    fn new(name: &str, schema: &Value, required: bool) -> Self {
        let string = |key| schema.get(key).and_then(Value::as_str);
        let type_ = schema.get("type").and_then(Value::as_str).unwrap_or("");
        let options = choice_options(schema);
        let kind = match (type_, options.is_empty()) {
            ("string" | "openai/imagePicker", false) => FieldKind::Choice(options, false),
            ("array", false) => FieldKind::Choice(options, true),
            ("string", _) => FieldKind::Text,
            ("number" | "integer", _) => FieldKind::Number(type_ == "integer"),
            ("boolean", _) => FieldKind::Boolean,
            _ => FieldKind::Json,
        };
        Self {
            name: name.to_string(),
            label: string("title").unwrap_or(name).to_string(),
            description: string("description").map(str::to_string),
            required,
            default: schema.get("default").cloned(),
            kind,
        }
    }
    fn parse(&self, answer: &str) -> Result<Option<Value>, String> {
        let answer = answer.trim();
        if answer.is_empty() {
            if let Some(default) = &self.default {
                return self
                    .kind
                    .parse(&value_input(default))
                    .map(Some)
                    .map_err(|message| format!("{} default: {message}", self.label));
            }
            return if self.required {
                Err("a value is required".to_string())
            } else {
                Ok(None)
            };
        }
        self.kind
            .parse(answer)
            .map(Some)
            .map_err(|message| format!("{}: {message}", self.label))
    }
    fn default_input(&self) -> Option<String> {
        let default = self.default.as_ref()?;
        if matches!(self.kind, FieldKind::Number(true))
            && let Some(value) = default.as_f64()
        {
            return Some(format!("{value:.0}"));
        }
        Some(value_input(default))
    }
}
impl FieldKind {
    fn parse(&self, answer: &str) -> Result<Value, String> {
        match self {
            Self::Text => Ok(Value::String(answer.to_string())),
            Self::Number(integer) => Ok(Value::Number(parse_number(answer, *integer)?)),
            Self::Boolean => match answer.to_ascii_lowercase().as_str() {
                "true" | "yes" | "y" | "1" => Ok(Value::Bool(true)),
                "false" | "no" | "n" | "0" => Ok(Value::Bool(false)),
                _ => Err("enter true or false".to_string()),
            },
            Self::Choice(options, false) => Ok(Value::String(selected_option(options, answer)?)),
            Self::Choice(options, true) => answer
                .split(',')
                .map(|input| selected_option(options, input.trim()).map(Value::String))
                .collect::<Result<_, _>>()
                .map(Value::Array),
            Self::Json => {
                Ok(serde_json::from_str(answer)
                    .unwrap_or_else(|_| Value::String(answer.to_string())))
            }
        }
    }
    fn hint(&self) -> String {
        match self {
            Self::Text => "text".to_string(),
            Self::Number(integer) => if *integer { "integer" } else { "number" }.to_string(),
            Self::Boolean => "true or false".to_string(),
            Self::Choice(options, multiple) => {
                let choices = options
                    .iter()
                    .enumerate()
                    .map(|(index, option)| format!("{} {}", index + 1, option.label))
                    .collect::<Vec<_>>()
                    .join(", ");
                if *multiple {
                    format!("comma-separated: {choices}")
                } else {
                    choices
                }
            }
            Self::Json => "JSON or text".to_string(),
        }
    }
}
fn choice_options(schema: &Value) -> Vec<OptionItem> {
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        let labels = schema.get("enumNames").and_then(Value::as_array);
        return values
            .iter()
            .enumerate()
            .filter_map(|(index, value)| {
                let value = value.as_str()?;
                let label = labels
                    .and_then(|labels| labels.get(index))
                    .and_then(Value::as_str)
                    .unwrap_or(value);
                Some(OptionItem {
                    value: value.to_string(),
                    label: label.to_string(),
                })
            })
            .collect();
    }
    if let Some(values) = schema.pointer("/items/enum").and_then(Value::as_array) {
        return values
            .iter()
            .filter_map(Value::as_str)
            .map(|value| OptionItem {
                value: value.to_string(),
                label: value.to_string(),
            })
            .collect();
    }
    ["/oneOf", "/items/anyOf", "/items"]
        .into_iter()
        .find_map(|pointer| schema.pointer(pointer).and_then(Value::as_array))
        .into_iter()
        .flatten()
        .filter_map(|option| {
            let value = option
                .get("const")
                .or_else(|| option.get("id"))
                .and_then(Value::as_str)?;
            let label = option.get("title").and_then(Value::as_str).unwrap_or(value);
            Some(OptionItem {
                value: value.to_string(),
                label: label.to_string(),
            })
        })
        .collect()
}
fn selected_option(options: &[OptionItem], input: &str) -> Result<String, String> {
    if let Ok(index) = input.parse::<usize>()
        && let Some(option) = index.checked_sub(1).and_then(|index| options.get(index))
    {
        return Ok(option.value.clone());
    }
    options
        .iter()
        .find(|option| option.value == input || option.label.eq_ignore_ascii_case(input))
        .map(|option| option.value.clone())
        .ok_or_else(|| "choose one of the listed options".to_string())
}
fn value_input(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        value => value.to_string(),
    }
}
fn parse_number(input: &str, integer: bool) -> Result<serde_json::Number, String> {
    let Ok(Value::Number(value)) = serde_json::from_str(input) else {
        return Err("enter a finite number".to_string());
    };
    if integer && !value.is_i64() && !value.is_u64() {
        let Some(value) = value.as_f64().filter(|value| value.fract() == 0.0) else {
            return Err("enter a whole number".to_string());
        };
        let value = format!("{value:.0}");
        return value
            .parse::<i64>()
            .map(serde_json::Number::from)
            .or_else(|_| value.parse::<u64>().map(serde_json::Number::from))
            .map_err(|_| "enter a whole number".to_string());
    }
    Ok(value)
}
