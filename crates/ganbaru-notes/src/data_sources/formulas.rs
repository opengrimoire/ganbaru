pub mod parser;

use self::parser::{
    BinaryOp, FormulaExpr, MAX_FORMULA_EXPRESSION_CHARS, UnaryOp, formula_dependencies,
    parse_formula_expression,
};
use crate::data_sources;
use crate::models::NotePageRow;
use serde_json::{Map, Number, Value, json};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

const MAX_PROPERTY_NAME_CHARS: usize = 120;
const MAX_PROPERTY_ID_CHARS: usize = 80;
const MAX_EVAL_DEPTH: usize = 64;

#[derive(Clone)]
struct SchemaProperty {
    key: String,
    id: String,
    name: String,
    property_type: String,
}

#[derive(Clone)]
struct FormulaProperty {
    key: String,
    id: String,
    name: String,
    ast: FormulaExpr,
    dependencies: HashSet<String>,
}

#[derive(Clone, Debug)]
enum FormulaValue {
    Empty,
    Number(f64),
    String(String),
    Boolean(bool),
    Date(Value),
    List(Vec<FormulaValue>),
}

pub fn canonical_formula_config(value: Option<&Value>) -> Result<Value, String> {
    let object = value
        .and_then(Value::as_object)
        .ok_or_else(|| "formula config must be an object".to_string())?;
    let expression = validate_expression(read_string_field(
        object,
        "expression",
        "formula.expression",
    )?)?;
    let ast = parse_formula_expression(&expression)?;
    formula_dependencies(&ast)?;
    Ok(json!({ "expression": expression }))
}

pub fn ensure_formula_schema(properties: &Value) -> Result<(), String> {
    formula_properties_from_schema(properties).map(|_| ())
}

pub fn hydrate_formulas(schema_properties: &Value, rows: &mut [NotePageRow]) -> Result<(), String> {
    let schema = schema_properties_from_schema(schema_properties)?;
    let formulas = formula_properties_from_schema(schema_properties)?;
    if formulas.is_empty() {
        return Ok(());
    }
    let schema_by_name = schema
        .iter()
        .map(|property| (property.name.to_lowercase(), property.clone()))
        .collect::<HashMap<_, _>>();
    for row in rows {
        let mut properties = parse_json(&row.properties, "row page properties")?;
        let properties_object = properties
            .as_object_mut()
            .ok_or_else(|| "row page properties must be an object".to_string())?;
        for formula in &formulas {
            let payload = match evaluate_formula(formula, properties_object, &schema_by_name) {
                Ok(value) => formula_payload(value),
                Err(message) => formula_error_payload(&message),
            };
            properties_object.insert(
                formula.key.clone(),
                formula_property_value(&formula.id, payload),
            );
        }
        row.properties = properties.to_string();
    }
    Ok(())
}

pub fn formula_property_value(property_id: &str, payload: Value) -> Value {
    json!({
        "id": property_id,
        "type": "formula",
        "formula": payload
    })
}

pub fn formula_plain_text(payload: &Value) -> String {
    match formula_value_from_formula_payload(payload) {
        FormulaValue::Empty => String::new(),
        value => value_to_text(&value),
    }
}

pub fn formula_number(payload: &Value) -> Option<f64> {
    match formula_value_from_formula_payload(payload) {
        FormulaValue::Number(value) if value.is_finite() => Some(value),
        _ => None,
    }
}

pub fn formula_checked(payload: &Value) -> Option<bool> {
    match formula_value_from_formula_payload(payload) {
        FormulaValue::Boolean(value) => Some(value),
        _ => None,
    }
}

fn formula_properties_from_schema(properties: &Value) -> Result<Vec<FormulaProperty>, String> {
    let schema = schema_properties_from_schema(properties)?;
    let properties_by_name = schema
        .iter()
        .map(|property| (property.name.to_lowercase(), property))
        .collect::<HashMap<_, _>>();
    let mut formulas = Vec::new();
    for property in &schema {
        if property.property_type != "formula" {
            continue;
        }
        let config = formula_config_from_schema(properties, property)?;
        for dependency_name in &config.dependencies {
            let dependency = properties_by_name
                .get(&dependency_name.to_lowercase())
                .ok_or_else(|| "formula references an unknown property".to_string())?;
            if dependency.id == property.id {
                return Err("formula cannot reference itself".to_string());
            }
        }
        formulas.push(config);
    }
    order_formulas(&formulas, &properties_by_name)
}

fn formula_config_from_schema(
    properties: &Value,
    property: &SchemaProperty,
) -> Result<FormulaProperty, String> {
    let schema = properties
        .get(&property.key)
        .and_then(Value::as_object)
        .ok_or_else(|| "formula property schema must be an object".to_string())?;
    let formula = schema
        .get("formula")
        .and_then(Value::as_object)
        .ok_or_else(|| "formula config must be an object".to_string())?;
    let expression = validate_expression(read_string_field(
        formula,
        "expression",
        "formula.expression",
    )?)?;
    let ast = parse_formula_expression(&expression)?;
    let dependencies = formula_dependencies(&ast)?;
    Ok(FormulaProperty {
        key: property.key.clone(),
        id: property.id.clone(),
        name: property.name.clone(),
        ast,
        dependencies,
    })
}

fn order_formulas(
    formulas: &[FormulaProperty],
    properties_by_name: &HashMap<String, &SchemaProperty>,
) -> Result<Vec<FormulaProperty>, String> {
    let formulas_by_id = formulas
        .iter()
        .map(|formula| (formula.id.clone(), formula))
        .collect::<HashMap<_, _>>();
    let mut formula_id_by_dependency_name = HashMap::new();
    for formula in formulas {
        formula_id_by_dependency_name.insert(formula.name.to_lowercase(), formula.id.clone());
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut ordered = Vec::new();
    for formula in formulas {
        visit_formula(
            formula,
            &formulas_by_id,
            &formula_id_by_dependency_name,
            properties_by_name,
            &mut visiting,
            &mut visited,
            &mut ordered,
        )?;
    }
    Ok(ordered)
}

fn visit_formula(
    formula: &FormulaProperty,
    formulas_by_id: &HashMap<String, &FormulaProperty>,
    formula_id_by_dependency_name: &HashMap<String, String>,
    properties_by_name: &HashMap<String, &SchemaProperty>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    ordered: &mut Vec<FormulaProperty>,
) -> Result<(), String> {
    if visited.contains(&formula.id) {
        return Ok(());
    }
    if !visiting.insert(formula.id.clone()) {
        return Err("formula dependency cycle is not allowed".to_string());
    }
    for dependency_name in &formula.dependencies {
        let dependency = properties_by_name
            .get(&dependency_name.to_lowercase())
            .ok_or_else(|| "formula references an unknown property".to_string())?;
        if dependency.property_type != "formula" {
            continue;
        }
        let dependency_formula_id = formula_id_by_dependency_name
            .get(&dependency.name.to_lowercase())
            .ok_or_else(|| "formula dependency is missing".to_string())?;
        let dependency_formula = formulas_by_id
            .get(dependency_formula_id)
            .copied()
            .ok_or_else(|| "formula dependency is missing".to_string())?;
        visit_formula(
            dependency_formula,
            formulas_by_id,
            formula_id_by_dependency_name,
            properties_by_name,
            visiting,
            visited,
            ordered,
        )?;
    }
    visiting.remove(&formula.id);
    visited.insert(formula.id.clone());
    ordered.push(formula.clone());
    Ok(())
}

fn schema_properties_from_schema(properties: &Value) -> Result<Vec<SchemaProperty>, String> {
    let object = properties
        .as_object()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    let mut result = Vec::with_capacity(object.len());
    for (key, value) in object {
        let property = value
            .as_object()
            .ok_or_else(|| "data source property must be an object".to_string())?;
        result.push(SchemaProperty {
            key: key.clone(),
            id: validate_non_empty_text(
                read_string_field(property, "id", "property.id")?,
                "property.id",
                MAX_PROPERTY_ID_CHARS,
            )?,
            name: validate_non_empty_text(
                read_string_field(property, "name", "property.name")?,
                "property.name",
                MAX_PROPERTY_NAME_CHARS,
            )?,
            property_type: read_string_field(property, "type", "property.type")?.to_string(),
        });
    }
    Ok(result)
}

fn evaluate_formula(
    formula: &FormulaProperty,
    properties: &Map<String, Value>,
    schema_by_name: &HashMap<String, SchemaProperty>,
) -> Result<FormulaValue, String> {
    evaluate_expr(&formula.ast, properties, schema_by_name, 0)
        .map_err(|message| format!("{}: {message}", formula.name))
}

fn evaluate_expr(
    expr: &FormulaExpr,
    properties: &Map<String, Value>,
    schema_by_name: &HashMap<String, SchemaProperty>,
    depth: usize,
) -> Result<FormulaValue, String> {
    if depth > MAX_EVAL_DEPTH {
        return Err("formula evaluation is too deep".to_string());
    }
    match expr {
        FormulaExpr::Number(value) => Ok(FormulaValue::Number(*value)),
        FormulaExpr::String(value) => Ok(FormulaValue::String(value.clone())),
        FormulaExpr::Boolean(value) => Ok(FormulaValue::Boolean(*value)),
        FormulaExpr::Unary { op, expr } => {
            let value = evaluate_expr(expr, properties, schema_by_name, depth + 1)?;
            match op {
                UnaryOp::Negate => Ok(FormulaValue::Number(-coerce_number(&value)?)),
                UnaryOp::Not => Ok(FormulaValue::Boolean(!truthy(&value))),
            }
        }
        FormulaExpr::Binary { op, left, right } => {
            evaluate_binary(*op, left, right, properties, schema_by_name, depth + 1)
        }
        FormulaExpr::Call { name, args } => {
            evaluate_call(name, args, properties, schema_by_name, depth + 1)
        }
    }
}

fn evaluate_binary(
    op: BinaryOp,
    left: &FormulaExpr,
    right: &FormulaExpr,
    properties: &Map<String, Value>,
    schema_by_name: &HashMap<String, SchemaProperty>,
    depth: usize,
) -> Result<FormulaValue, String> {
    if op == BinaryOp::And {
        let left_value = evaluate_expr(left, properties, schema_by_name, depth)?;
        if !truthy(&left_value) {
            return Ok(FormulaValue::Boolean(false));
        }
        let right_value = evaluate_expr(right, properties, schema_by_name, depth)?;
        return Ok(FormulaValue::Boolean(truthy(&right_value)));
    }
    if op == BinaryOp::Or {
        let left_value = evaluate_expr(left, properties, schema_by_name, depth)?;
        if truthy(&left_value) {
            return Ok(FormulaValue::Boolean(true));
        }
        let right_value = evaluate_expr(right, properties, schema_by_name, depth)?;
        return Ok(FormulaValue::Boolean(truthy(&right_value)));
    }
    let left_value = evaluate_expr(left, properties, schema_by_name, depth)?;
    let right_value = evaluate_expr(right, properties, schema_by_name, depth)?;
    match op {
        BinaryOp::Add => {
            if matches!(left_value, FormulaValue::String(_))
                || matches!(right_value, FormulaValue::String(_))
            {
                return Ok(FormulaValue::String(format!(
                    "{}{}",
                    value_to_text(&left_value),
                    value_to_text(&right_value)
                )));
            }
            Ok(FormulaValue::Number(
                coerce_number(&left_value)? + coerce_number(&right_value)?,
            ))
        }
        BinaryOp::Subtract => Ok(FormulaValue::Number(
            coerce_number(&left_value)? - coerce_number(&right_value)?,
        )),
        BinaryOp::Multiply => Ok(FormulaValue::Number(
            coerce_number(&left_value)? * coerce_number(&right_value)?,
        )),
        BinaryOp::Divide => {
            let right_number = coerce_number(&right_value)?;
            if right_number == 0.0 {
                return Err("division by zero".to_string());
            }
            Ok(FormulaValue::Number(
                coerce_number(&left_value)? / right_number,
            ))
        }
        BinaryOp::Modulo => {
            let right_number = coerce_number(&right_value)?;
            if right_number == 0.0 {
                return Err("division by zero".to_string());
            }
            Ok(FormulaValue::Number(
                coerce_number(&left_value)? % right_number,
            ))
        }
        BinaryOp::Equal => Ok(FormulaValue::Boolean(values_equal(
            &left_value,
            &right_value,
        ))),
        BinaryOp::NotEqual => Ok(FormulaValue::Boolean(!values_equal(
            &left_value,
            &right_value,
        ))),
        BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => {
            compare_values(op, &left_value, &right_value)
        }
        BinaryOp::And | BinaryOp::Or => unreachable!(),
    }
}

fn evaluate_call(
    name: &str,
    args: &[FormulaExpr],
    properties: &Map<String, Value>,
    schema_by_name: &HashMap<String, SchemaProperty>,
    depth: usize,
) -> Result<FormulaValue, String> {
    let name = name.to_ascii_lowercase();
    match name.as_str() {
        "prop" => evaluate_prop(args, properties, schema_by_name),
        "if" => evaluate_if(args, properties, schema_by_name, depth),
        "empty" => {
            let values = evaluate_args(args, properties, schema_by_name, depth)?;
            let [value] = values.as_slice() else {
                return Err("empty() expects one argument".to_string());
            };
            Ok(FormulaValue::Boolean(is_empty(value)))
        }
        "format" => {
            let values = evaluate_args(args, properties, schema_by_name, depth)?;
            let [value] = values.as_slice() else {
                return Err("format() expects one argument".to_string());
            };
            Ok(FormulaValue::String(value_to_text(value)))
        }
        "length" => {
            let values = evaluate_args(args, properties, schema_by_name, depth)?;
            let [value] = values.as_slice() else {
                return Err("length() expects one argument".to_string());
            };
            let length = match value {
                FormulaValue::List(items) => items.len(),
                _ => value_to_text(value).chars().count(),
            };
            Ok(FormulaValue::Number(length as f64))
        }
        "contains" => {
            let values = evaluate_args(args, properties, schema_by_name, depth)?;
            let [value, search] = values.as_slice() else {
                return Err("contains() expects two arguments".to_string());
            };
            Ok(FormulaValue::Boolean(
                value_to_text(value).contains(&value_to_text(search)),
            ))
        }
        "lower" | "upper" => {
            let values = evaluate_args(args, properties, schema_by_name, depth)?;
            let [value] = values.as_slice() else {
                return Err(format!("{name}() expects one argument"));
            };
            let text = value_to_text(value);
            Ok(FormulaValue::String(if name == "lower" {
                text.to_lowercase()
            } else {
                text.to_uppercase()
            }))
        }
        "abs" | "round" | "floor" | "ceil" => {
            let values = evaluate_args(args, properties, schema_by_name, depth)?;
            let [value] = values.as_slice() else {
                return Err(format!("{name}() expects one argument"));
            };
            let number = coerce_number(value)?;
            let result = match name.as_str() {
                "abs" => number.abs(),
                "round" => number.round(),
                "floor" => number.floor(),
                _ => number.ceil(),
            };
            Ok(FormulaValue::Number(result))
        }
        "min" | "max" => {
            let values = evaluate_args(args, properties, schema_by_name, depth)?;
            if values.is_empty() {
                return Err(format!("{name}() expects at least one argument"));
            }
            let mut numbers = values.iter().map(coerce_number);
            let first = numbers
                .next()
                .ok_or_else(|| format!("{name}() expects at least one argument"))??;
            let result = numbers.try_fold(first, |current, value| {
                let value = value?;
                Ok::<_, String>(if name == "min" {
                    current.min(value)
                } else {
                    current.max(value)
                })
            })?;
            Ok(FormulaValue::Number(result))
        }
        "add" | "subtract" | "multiply" | "divide" | "mod" => {
            let values = evaluate_args(args, properties, schema_by_name, depth)?;
            let [left, right] = values.as_slice() else {
                return Err(format!("{name}() expects two arguments"));
            };
            let op = match name.as_str() {
                "add" => BinaryOp::Add,
                "subtract" => BinaryOp::Subtract,
                "multiply" => BinaryOp::Multiply,
                "divide" => BinaryOp::Divide,
                _ => BinaryOp::Modulo,
            };
            evaluate_binary(
                op,
                &literal_expr(left.clone()),
                &literal_expr(right.clone()),
                properties,
                schema_by_name,
                depth,
            )
        }
        "not" => {
            let values = evaluate_args(args, properties, schema_by_name, depth)?;
            let [value] = values.as_slice() else {
                return Err("not() expects one argument".to_string());
            };
            Ok(FormulaValue::Boolean(!truthy(value)))
        }
        "and" | "or" => {
            let values = evaluate_args(args, properties, schema_by_name, depth)?;
            if values.len() != 2 {
                return Err(format!("{name}() expects two arguments"));
            }
            let left = truthy(&values[0]);
            let right = truthy(&values[1]);
            Ok(FormulaValue::Boolean(if name == "and" {
                left && right
            } else {
                left || right
            }))
        }
        _ => Err(format!("unsupported formula function: {name}")),
    }
}

fn evaluate_prop(
    args: &[FormulaExpr],
    properties: &Map<String, Value>,
    schema_by_name: &HashMap<String, SchemaProperty>,
) -> Result<FormulaValue, String> {
    let [FormulaExpr::String(property_name)] = args else {
        return Err("prop() requires one literal property name".to_string());
    };
    let property = schema_by_name
        .get(&property_name.to_lowercase())
        .ok_or_else(|| "prop() references an unknown property".to_string())?;
    let Some(value) = properties.get(&property.key).or_else(|| {
        properties.values().find(|value| {
            value.get("id").and_then(Value::as_str) == Some(property.id.as_str())
                && value.get("type").and_then(Value::as_str)
                    == Some(property.property_type.as_str())
        })
    }) else {
        return Ok(FormulaValue::Empty);
    };
    Ok(property_value_to_formula_value(value))
}

fn evaluate_if(
    args: &[FormulaExpr],
    properties: &Map<String, Value>,
    schema_by_name: &HashMap<String, SchemaProperty>,
    depth: usize,
) -> Result<FormulaValue, String> {
    let [condition, truthy_expr, falsy_expr] = args else {
        return Err("if() expects three arguments".to_string());
    };
    let condition = evaluate_expr(condition, properties, schema_by_name, depth)?;
    if truthy(&condition) {
        evaluate_expr(truthy_expr, properties, schema_by_name, depth)
    } else {
        evaluate_expr(falsy_expr, properties, schema_by_name, depth)
    }
}

fn evaluate_args(
    args: &[FormulaExpr],
    properties: &Map<String, Value>,
    schema_by_name: &HashMap<String, SchemaProperty>,
    depth: usize,
) -> Result<Vec<FormulaValue>, String> {
    args.iter()
        .map(|arg| evaluate_expr(arg, properties, schema_by_name, depth))
        .collect()
}

fn literal_expr(value: FormulaValue) -> FormulaExpr {
    match value {
        FormulaValue::Number(value) => FormulaExpr::Number(value),
        FormulaValue::String(value) => FormulaExpr::String(value),
        FormulaValue::Boolean(value) => FormulaExpr::Boolean(value),
        FormulaValue::Date(value) => FormulaExpr::String(date_payload_text(&value)),
        FormulaValue::List(value) => FormulaExpr::String(
            value
                .iter()
                .map(value_to_text)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        FormulaValue::Empty => FormulaExpr::String(String::new()),
    }
}

fn property_value_to_formula_value(value: &Value) -> FormulaValue {
    let property_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let payload = value.get(property_type).unwrap_or(&Value::Null);
    match property_type {
        "title" | "rich_text" => payload
            .as_array()
            .map(|items| FormulaValue::String(rich_text_plain_text(items)))
            .unwrap_or(FormulaValue::Empty),
        "number" => payload
            .as_f64()
            .map(FormulaValue::Number)
            .unwrap_or(FormulaValue::Empty),
        "checkbox" => payload
            .as_bool()
            .map(FormulaValue::Boolean)
            .unwrap_or(FormulaValue::Boolean(false)),
        "select" | "status" | "place" => payload
            .get("name")
            .and_then(Value::as_str)
            .map(|value| FormulaValue::String(value.to_string()))
            .unwrap_or(FormulaValue::Empty),
        "multi_select" | "people" | "relation" | "files" => payload
            .as_array()
            .map(|items| {
                FormulaValue::List(
                    items
                        .iter()
                        .filter_map(|item| {
                            item.get("title")
                                .or_else(|| item.get("name"))
                                .or_else(|| item.get("id"))
                                .and_then(Value::as_str)
                                .map(|value| FormulaValue::String(value.to_string()))
                        })
                        .collect(),
                )
            })
            .unwrap_or(FormulaValue::List(Vec::new())),
        "date" => {
            if payload.is_object() {
                FormulaValue::Date(payload.clone())
            } else {
                FormulaValue::Empty
            }
        }
        "url" | "email" | "phone_number" | "created_time" | "last_edited_time" => payload
            .as_str()
            .map(|value| FormulaValue::String(value.to_string()))
            .unwrap_or(FormulaValue::Empty),
        "unique_id" => FormulaValue::String(unique_id_text(payload)),
        "formula" => formula_value_from_formula_payload(payload),
        "rollup" => rollup_value_to_formula_value(payload),
        _ => FormulaValue::Empty,
    }
}

fn formula_value_from_formula_payload(payload: &Value) -> FormulaValue {
    match payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "number" => payload
            .get("number")
            .and_then(Value::as_f64)
            .map(FormulaValue::Number)
            .unwrap_or(FormulaValue::Empty),
        "boolean" => payload
            .get("boolean")
            .and_then(Value::as_bool)
            .map(FormulaValue::Boolean)
            .unwrap_or(FormulaValue::Boolean(false)),
        "date" => payload
            .get("date")
            .filter(|value| value.is_object())
            .cloned()
            .map(FormulaValue::Date)
            .unwrap_or(FormulaValue::Empty),
        "string" => payload
            .get("string")
            .and_then(Value::as_str)
            .map(|value| FormulaValue::String(value.to_string()))
            .unwrap_or(FormulaValue::Empty),
        _ => FormulaValue::Empty,
    }
}

fn rollup_value_to_formula_value(payload: &Value) -> FormulaValue {
    match payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "number" => payload
            .get("number")
            .and_then(Value::as_f64)
            .map(FormulaValue::Number)
            .unwrap_or(FormulaValue::Empty),
        "date" => payload
            .get("date")
            .filter(|value| value.is_object())
            .cloned()
            .map(FormulaValue::Date)
            .unwrap_or(FormulaValue::Empty),
        "array" => FormulaValue::String(data_sources::rollups::rollup_plain_text(payload)),
        _ => FormulaValue::Empty,
    }
}

fn formula_payload(value: FormulaValue) -> Value {
    match value {
        FormulaValue::Number(value) => json!({
            "type": "number",
            "number": finite_json_number(value).map(Value::Number).unwrap_or(Value::Null)
        }),
        FormulaValue::Boolean(value) => json!({
            "type": "boolean",
            "boolean": value
        }),
        FormulaValue::Date(value) => json!({
            "type": "date",
            "date": value
        }),
        FormulaValue::String(value) => json!({
            "type": "string",
            "string": value
        }),
        FormulaValue::List(values) => json!({
            "type": "string",
            "string": values.iter().map(value_to_text).collect::<Vec<_>>().join(", ")
        }),
        FormulaValue::Empty => json!({
            "type": "string",
            "string": ""
        }),
    }
}

fn formula_error_payload(message: &str) -> Value {
    json!({
        "type": "string",
        "string": format!("Formula error: {message}"),
        "ganbaru_error": message
    })
}

fn compare_values(
    op: BinaryOp,
    left: &FormulaValue,
    right: &FormulaValue,
) -> Result<FormulaValue, String> {
    let ordering =
        if matches!(left, FormulaValue::Number(_)) || matches!(right, FormulaValue::Number(_)) {
            coerce_number(left)?
                .partial_cmp(&coerce_number(right)?)
                .unwrap_or(Ordering::Equal)
        } else {
            value_to_text(left).cmp(&value_to_text(right))
        };
    let result = match op {
        BinaryOp::Less => ordering == Ordering::Less,
        BinaryOp::LessEqual => ordering != Ordering::Greater,
        BinaryOp::Greater => ordering == Ordering::Greater,
        BinaryOp::GreaterEqual => ordering != Ordering::Less,
        _ => return Err("invalid comparison operator".to_string()),
    };
    Ok(FormulaValue::Boolean(result))
}

fn values_equal(left: &FormulaValue, right: &FormulaValue) -> bool {
    match (left, right) {
        (FormulaValue::Empty, FormulaValue::Empty) => true,
        (FormulaValue::Number(left), FormulaValue::Number(right)) => {
            left.partial_cmp(right) == Some(Ordering::Equal)
        }
        (FormulaValue::Boolean(left), FormulaValue::Boolean(right)) => left == right,
        (FormulaValue::Date(left), FormulaValue::Date(right)) => left == right,
        _ => value_to_text(left) == value_to_text(right),
    }
}

fn coerce_number(value: &FormulaValue) -> Result<f64, String> {
    let number = match value {
        FormulaValue::Number(value) => *value,
        FormulaValue::Boolean(value) => {
            if *value {
                1.0
            } else {
                0.0
            }
        }
        FormulaValue::String(value) if value.trim().is_empty() => 0.0,
        FormulaValue::String(value) => value
            .trim()
            .parse::<f64>()
            .map_err(|_| "formula value is not numeric".to_string())?,
        FormulaValue::Empty => 0.0,
        FormulaValue::Date(_) | FormulaValue::List(_) => {
            return Err("formula value is not numeric".to_string());
        }
    };
    if number.is_finite() {
        Ok(number)
    } else {
        Err("formula value must be finite".to_string())
    }
}

fn truthy(value: &FormulaValue) -> bool {
    match value {
        FormulaValue::Boolean(value) => *value,
        FormulaValue::Number(value) => *value != 0.0,
        FormulaValue::String(value) => !value.trim().is_empty(),
        FormulaValue::Date(_) => true,
        FormulaValue::List(values) => !values.is_empty(),
        FormulaValue::Empty => false,
    }
}

fn is_empty(value: &FormulaValue) -> bool {
    match value {
        FormulaValue::Empty => true,
        FormulaValue::Number(value) => *value == 0.0,
        FormulaValue::String(value) => value.trim().is_empty(),
        FormulaValue::List(values) => values.is_empty(),
        FormulaValue::Boolean(_) | FormulaValue::Date(_) => false,
    }
}

fn value_to_text(value: &FormulaValue) -> String {
    match value {
        FormulaValue::Empty => String::new(),
        FormulaValue::Number(value) => number_to_text(*value),
        FormulaValue::String(value) => value.clone(),
        FormulaValue::Boolean(value) => value.to_string(),
        FormulaValue::Date(value) => date_payload_text(value),
        FormulaValue::List(values) => values
            .iter()
            .map(value_to_text)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn rich_text_plain_text(items: &[Value]) -> String {
    let mut text = String::new();
    for item in items {
        if let Some(plain_text) = item.get("plain_text").and_then(Value::as_str) {
            text.push_str(plain_text);
        } else if let Some(content) = item
            .get("text")
            .and_then(|value| value.get("content"))
            .and_then(Value::as_str)
        {
            text.push_str(content);
        }
    }
    text
}

fn unique_id_text(payload: &Value) -> String {
    let prefix = payload.get("prefix").and_then(Value::as_str).unwrap_or("");
    let number = payload
        .get("number")
        .and_then(Value::as_i64)
        .map(|value| value.to_string())
        .unwrap_or_default();
    format!("{prefix}{number}")
}

fn date_payload_text(value: &Value) -> String {
    let Some(object) = value.as_object() else {
        return String::new();
    };
    let start = object
        .get("start")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let end = object
        .get("end")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if end.is_empty() || end == start {
        start.to_string()
    } else {
        format!("{start} to {end}")
    }
}

fn number_to_text(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn finite_json_number(value: f64) -> Option<Number> {
    if value.is_finite() {
        Number::from_f64(value)
    } else {
        None
    }
}

fn validate_expression(value: &str) -> Result<String, String> {
    if value.chars().any(char::is_control) {
        return Err("formula.expression must not contain control characters".to_string());
    }
    if value.chars().count() > MAX_FORMULA_EXPRESSION_CHARS {
        return Err("formula.expression is too long".to_string());
    }
    let expression = value.trim().to_string();
    if expression.is_empty() {
        return Err("formula.expression is required".to_string());
    }
    Ok(expression)
}

fn validate_non_empty_text(value: &str, label: &str, max_chars: usize) -> Result<String, String> {
    let text = value.trim();
    if text.is_empty() {
        return Err(format!("{label} is required"));
    }
    if text.chars().any(char::is_control) {
        return Err(format!("{label} must not contain control characters"));
    }
    if text.chars().count() > max_chars {
        return Err(format!("{label} is too long"));
    }
    Ok(text.to_string())
}

fn read_string_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{label} must be a string"))
}

fn parse_json(value: &str, label: &str) -> Result<Value, String> {
    serde_json::from_str(value).map_err(|e| format!("parse {label}: {e}"))
}
