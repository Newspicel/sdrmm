use std::sync::Arc;

use rmcp::{
    ErrorData,
    handler::server::{common::FromContextPart, tool::ToolCallContext},
    model::JsonObject,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use utoipa::{
    ToSchema,
    openapi::{RefOr, schema::Schema},
};

const OPENAPI_REFS: &str = "#/components/schemas/";
const JSON_SCHEMA_REFS: &str = "#/$defs/";

pub(super) struct Args<T>(pub T);

impl<S, T: DeserializeOwned> FromContextPart<ToolCallContext<'_, S>> for Args<T> {
    fn from_context_part(context: &mut ToolCallContext<'_, S>) -> Result<Self, ErrorData> {
        let arguments = Value::Object(context.arguments.take().unwrap_or_default());
        crate::json::from_value(&arguments)
            .map(Self)
            .map_err(|err| ErrorData::invalid_params(format!("unusable arguments: {err}"), None))
    }
}

pub(super) fn input<T: ToSchema>() -> Arc<JsonObject> {
    let mut nested = Vec::new();
    T::schemas(&mut nested);
    let mut root = object(&T::schema());
    if !nested.is_empty() {
        let defs = nested
            .iter()
            .map(|(name, schema)| (name.clone(), Value::Object(object(schema))))
            .collect();
        root.insert("$defs".to_owned(), Value::Object(defs));
    }
    root.entry("type")
        .or_insert_with(|| Value::String("object".to_owned()));
    retarget_object(&mut root);
    Arc::new(root)
}

fn object(schema: &RefOr<Schema>) -> JsonObject {
    match serde_json::to_value(schema) {
        Ok(Value::Object(object)) => object,
        Ok(other) => panic!("a schema serialized as {other}, not an object"),
        Err(err) => panic!("a schema did not serialize: {err}"),
    }
}

fn retarget_object(object: &mut JsonObject) {
    for (key, inner) in object.iter_mut() {
        match inner {
            Value::String(target) if key == "$ref" => {
                if let Some(name) = target.strip_prefix(OPENAPI_REFS) {
                    *target = format!("{JSON_SCHEMA_REFS}{name}");
                }
            }
            _ => retarget(inner),
        }
    }
}

fn retarget(value: &mut Value) {
    match value {
        Value::Object(object) => retarget_object(object),
        Value::Array(items) => items.iter_mut().for_each(retarget),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(value: &Value, found: &mut Vec<String>) {
        match value {
            Value::Object(object) => {
                for (key, inner) in object {
                    match inner {
                        Value::String(target) if key == "$ref" => found.push(target.clone()),
                        _ => refs(inner, found),
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|item| refs(item, found)),
            _ => {}
        }
    }

    #[test]
    fn every_reference_lands_in_the_schema_itself() {
        let schema = Value::Object(input::<sdrmm_wire::PatchNode>().as_ref().clone());
        let mut found = Vec::new();
        refs(&schema, &mut found);
        assert!(!found.is_empty());
        for target in found {
            let name = target
                .strip_prefix(JSON_SCHEMA_REFS)
                .unwrap_or_else(|| panic!("{target} points outside the schema"));
            assert!(schema["$defs"][name].is_object(), "{name} is not defined");
        }
    }
}
