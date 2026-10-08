use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};

const KEYS: [&str; 2] = ["id", "node"];
const TAGS: [&str; 2] = ["kind", "type"];

pub(crate) fn merge3(base: &Value, ours: &Value, theirs: &Value) -> Value {
    if ours == base || ours == theirs {
        return theirs.clone();
    }
    if theirs == base {
        return ours.clone();
    }
    match (base, ours, theirs) {
        (Value::Object(base), Value::Object(ours), Value::Object(theirs))
            if same_variant(ours, theirs) =>
        {
            Value::Object(merge_objects(base, ours, theirs))
        }
        (Value::Array(base), Value::Array(ours), Value::Array(theirs)) => {
            merge_arrays(base, ours, theirs)
                .map_or_else(|| Value::Array(ours.clone()), Value::Array)
        }
        _ => ours.clone(),
    }
}

pub(crate) fn merge_typed<T: Serialize + DeserializeOwned>(
    base: &T,
    ours: &T,
    theirs: &T,
) -> Result<T, serde_json::Error> {
    let merged = merge3(
        &serde_json::to_value(base)?,
        &serde_json::to_value(ours)?,
        &serde_json::to_value(theirs)?,
    );
    serde_json::from_value(merged)
}

fn merge_objects(
    base: &Map<String, Value>,
    ours: &Map<String, Value>,
    theirs: &Map<String, Value>,
) -> Map<String, Value> {
    let mut merged = Map::new();
    for key in theirs.keys().chain(ours.keys()) {
        if merged.contains_key(key) {
            continue;
        }
        if let Some(value) = merge_slot(base.get(key), ours.get(key), theirs.get(key)) {
            merged.insert(key.clone(), value);
        }
    }
    merged
}

fn merge_slot(base: Option<&Value>, ours: Option<&Value>, theirs: Option<&Value>) -> Option<Value> {
    if ours == base || ours == theirs {
        return theirs.cloned();
    }
    if theirs == base {
        return ours.cloned();
    }
    match (base, ours, theirs) {
        (Some(base), Some(ours), Some(theirs)) => Some(merge3(base, ours, theirs)),
        (Some(_), _, _) => None,
        (None, ours, _) => ours.cloned(),
    }
}

fn same_variant(ours: &Map<String, Value>, theirs: &Map<String, Value>) -> bool {
    TAGS.iter().all(|tag| ours.get(*tag) == theirs.get(*tag))
}

fn merge_arrays(base: &[Value], ours: &[Value], theirs: &[Value]) -> Option<Vec<Value>> {
    if let Some(key) = shared_key(base, ours, theirs) {
        return Some(merge_keyed(key, base, ours, theirs));
    }
    all_objects(base, ours, theirs).then(|| merge_sets(base, ours, theirs))
}

fn shared_key(base: &[Value], ours: &[Value], theirs: &[Value]) -> Option<&'static str> {
    KEYS.into_iter().find(|key| {
        base.iter()
            .chain(ours)
            .chain(theirs)
            .all(|item| item.get(key).is_some_and(Value::is_string))
    })
}

fn all_objects(base: &[Value], ours: &[Value], theirs: &[Value]) -> bool {
    base.iter().chain(ours).chain(theirs).all(Value::is_object)
}

fn find<'a>(items: &'a [Value], key: &str, id: &Value) -> Option<&'a Value> {
    items.iter().find(|item| item.get(key) == Some(id))
}

fn merge_keyed(key: &str, base: &[Value], ours: &[Value], theirs: &[Value]) -> Vec<Value> {
    let mut merged = Vec::new();
    let mut seen = Vec::new();
    for item in theirs.iter().chain(ours) {
        let Some(id) = item.get(key) else { continue };
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        let slot = merge_slot(
            find(base, key, id),
            find(ours, key, id),
            find(theirs, key, id),
        );
        merged.extend(slot);
    }
    merged
}

fn merge_sets(base: &[Value], ours: &[Value], theirs: &[Value]) -> Vec<Value> {
    let removed: Vec<&Value> = base.iter().filter(|item| !ours.contains(item)).collect();
    let mut merged: Vec<Value> = theirs
        .iter()
        .filter(|item| !removed.contains(item))
        .cloned()
        .collect();
    for item in ours {
        if !base.contains(item) && !merged.contains(item) {
            merged.push(item.clone());
        }
    }
    merged
}

#[cfg(test)]
mod tests;
