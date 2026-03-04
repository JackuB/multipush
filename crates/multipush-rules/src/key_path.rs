use serde_json::Value;

/// Traverse a `serde_json::Value` by dot-separated path segments.
pub fn get_by_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = root;
    for segment in path.split('.') {
        current = current.as_object()?.get(segment)?;
    }
    Some(current)
}

/// Set a value at a dot-separated path, creating intermediate objects as needed.
/// Returns `false` if a non-object intermediate is encountered and cannot be replaced.
pub fn set_by_path(root: &mut Value, path: &str, value: Value) -> bool {
    let segments: Vec<&str> = path.split('.').collect();
    let mut current = root;

    for &segment in &segments[..segments.len() - 1] {
        if !current.is_object() {
            return false;
        }
        let obj = current.as_object_mut().unwrap();
        if !obj.contains_key(segment) {
            obj.insert(segment.to_string(), Value::Object(serde_json::Map::new()));
        }
        current = obj.get_mut(segment).unwrap();
        if !current.is_object() {
            return false;
        }
    }

    if let Some(obj) = current.as_object_mut() {
        let last = segments.last().unwrap();
        obj.insert((*last).to_string(), value);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn get_shallow_key() {
        let v = json!({"name": "test"});
        assert_eq!(get_by_path(&v, "name"), Some(&json!("test")));
    }

    #[test]
    fn get_nested_key() {
        let v = json!({"a": {"b": {"c": 42}}});
        assert_eq!(get_by_path(&v, "a.b.c"), Some(&json!(42)));
    }

    #[test]
    fn get_missing_key() {
        let v = json!({"a": 1});
        assert_eq!(get_by_path(&v, "b"), None);
    }

    #[test]
    fn get_non_object_intermediate() {
        let v = json!({"a": "string"});
        assert_eq!(get_by_path(&v, "a.b"), None);
    }

    #[test]
    fn set_shallow_key() {
        let mut v = json!({"name": "old"});
        assert!(set_by_path(&mut v, "name", json!("new")));
        assert_eq!(v, json!({"name": "new"}));
    }

    #[test]
    fn set_creates_intermediates() {
        let mut v = json!({});
        assert!(set_by_path(&mut v, "a.b.c", json!(true)));
        assert_eq!(v, json!({"a": {"b": {"c": true}}}));
    }

    #[test]
    fn set_overwrites_existing() {
        let mut v = json!({"a": {"b": 1}});
        assert!(set_by_path(&mut v, "a.b", json!(2)));
        assert_eq!(v, json!({"a": {"b": 2}}));
    }

    #[test]
    fn set_non_object_intermediate_fails() {
        let mut v = json!({"a": "string"});
        assert!(!set_by_path(&mut v, "a.b", json!(1)));
    }
}
