pub mod builtin;

use std::collections::HashMap;

use indexmap::IndexMap;
use regex::Regex;
use serde_yaml_ng::{Mapping, Value};

use crate::error::CoreError;
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamType {
    String,
    Int,
}

impl std::fmt::Display for ParamType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParamType::String => write!(f, "string"),
            ParamType::Int => write!(f, "int"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParamDef {
    pub param_type: ParamType,
    pub required: bool,
    pub default: Option<String>,
    pub description: Option<String>,
    pub enum_values: Option<Vec<String>>,
}

pub struct Recipe {
    pub name: String,
    pub description: String,
    pub params: IndexMap<String, ParamDef>,
    pub lookups: HashMap<String, HashMap<String, String>>,
    raw_yaml: String,
}

impl Recipe {
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        let value: Value = serde_yaml_ng::from_str(yaml)
            .map_err(|e| CoreError::Recipe(format!("failed to parse recipe YAML: {e}")))?;

        let mapping = value
            .as_mapping()
            .ok_or_else(|| CoreError::Recipe("recipe must be a YAML mapping".into()))?;

        let name = get_string(mapping, "name")
            .ok_or_else(|| CoreError::Recipe("recipe missing 'name' field".into()))?;
        let description = get_string(mapping, "description")
            .ok_or_else(|| CoreError::Recipe(format!("recipe '{name}' missing 'description'")))?;

        let params = parse_params(mapping, &name)?;
        let lookups = parse_lookups(mapping, &name)?;

        // Validate that lookup keys reference existing params
        for key in lookups.keys() {
            if !params.contains_key(key) {
                return Err(CoreError::Recipe(format!(
                    "recipe '{name}': lookup key '{key}' does not match any parameter"
                )));
            }
        }

        Ok(Recipe {
            name,
            description,
            params,
            lookups,
            raw_yaml: yaml.to_string(),
        })
    }

    pub fn expand(&self, user_params: &HashMap<String, String>) -> Result<Value> {
        // 1. Validate required params
        for (name, def) in &self.params {
            if def.required && !user_params.contains_key(name) && def.default.is_none() {
                return Err(CoreError::Recipe(format!(
                    "recipe '{}': missing required parameter '{name}'",
                    self.name
                )));
            }
        }

        // 2. Reject unknown params
        for key in user_params.keys() {
            if !self.params.contains_key(key) {
                return Err(CoreError::Recipe(format!(
                    "recipe '{}': unknown parameter '{key}'",
                    self.name
                )));
            }
        }

        // 3. Validate enum constraints
        for (name, value) in user_params {
            if let Some(def) = self.params.get(name) {
                if let Some(enum_vals) = &def.enum_values {
                    if !enum_vals.contains(value) {
                        return Err(CoreError::Recipe(format!(
                            "recipe '{}': parameter '{name}' value '{value}' not in allowed values: {}",
                            self.name,
                            enum_vals.join(", ")
                        )));
                    }
                }
            }
        }

        // 4. Build resolved param map (user values + defaults)
        let mut resolved: HashMap<String, String> = HashMap::new();
        for (name, def) in &self.params {
            if let Some(val) = user_params.get(name) {
                resolved.insert(name.clone(), val.clone());
            } else if let Some(default) = &def.default {
                resolved.insert(name.clone(), default.clone());
            }
        }

        // 5. Apply lookups
        for (param_name, lookup_map) in &self.lookups {
            if let Some(value) = resolved.get(param_name).cloned() {
                if let Some(content) = lookup_map.get(&value) {
                    resolved.insert(param_name.clone(), content.clone());
                }
            }
        }

        // 5b. Resolve inter-param references in values (one pass)
        let re = Regex::new(r"\{\{\s*(\w+)\s*\}\}").unwrap();
        let snapshot = resolved.clone();
        for value in resolved.values_mut() {
            if value.contains("{{") {
                *value = re
                    .replace_all(value, |caps: &regex::Captures| {
                        let key = &caps[1];
                        snapshot.get(key).map_or("", |v| v.as_str()).to_string()
                    })
                    .into_owned();
            }
        }

        // 6. Template substitution on raw YAML
        let substituted = template_substitute(&self.raw_yaml, &resolved);

        // 7. Parse and extract rules
        let parsed: Value = serde_yaml_ng::from_str(&substituted).map_err(|e| {
            CoreError::Recipe(format!(
                "recipe '{}': failed to parse after template substitution: {e}",
                self.name
            ))
        })?;

        let parsed_map = parsed.as_mapping().ok_or_else(|| {
            CoreError::Recipe(format!(
                "recipe '{}': substituted YAML is not a mapping",
                self.name
            ))
        })?;

        let rules = parsed_map
            .get(Value::String("rules".into()))
            .ok_or_else(|| {
                CoreError::Recipe(format!(
                    "recipe '{}': no 'rules' key after expansion",
                    self.name
                ))
            })?
            .clone();

        // 8. Build result
        let mut result = Mapping::new();
        result.insert(
            Value::String("name".into()),
            Value::String(self.name.clone()),
        );
        result.insert(
            Value::String("description".into()),
            Value::String(self.description.clone()),
        );
        result.insert(Value::String("rules".into()), rules);

        Ok(Value::Mapping(result))
    }
}

/// Substitute `{{ param }}` placeholders in a template string.
///
/// For multiline replacement values, subsequent lines are indented to match
/// the leading whitespace of the line containing the placeholder.
fn template_substitute(template: &str, params: &HashMap<String, String>) -> String {
    let re = Regex::new(r"\{\{\s*(\w+)\s*\}\}").unwrap();

    let mut result = String::with_capacity(template.len());
    let mut last_end = 0;

    for caps in re.captures_iter(template) {
        let m = caps.get(0).unwrap();
        let key = &caps[1];
        let value = params.get(key).map_or("", |v| v.as_str());

        // Append text before this match
        result.push_str(&template[last_end..m.start()]);

        if value.contains('\n') {
            // Find the indentation level of this line
            let line_start = template[..m.start()].rfind('\n').map_or(0, |n| n + 1);
            let prefix = &template[line_start..m.start()];
            let indent: String = prefix
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect();

            // Indent subsequent lines of the replacement value
            let mut lines = value.split('\n');
            if let Some(first) = lines.next() {
                result.push_str(first);
                for line in lines {
                    result.push('\n');
                    if !line.is_empty() {
                        result.push_str(&indent);
                    }
                    result.push_str(line);
                }
            }
        } else {
            result.push_str(value);
        }

        last_end = m.end();
    }

    result.push_str(&template[last_end..]);
    result
}

fn get_string(mapping: &Mapping, key: &str) -> Option<String> {
    mapping
        .get(Value::String(key.into()))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn parse_params(
    mapping: &Mapping,
    recipe_name: &str,
) -> Result<IndexMap<String, ParamDef>> {
    let mut params = IndexMap::new();

    let params_val = match mapping.get(Value::String("params".into())) {
        Some(v) => v,
        None => return Ok(params),
    };

    let params_map = params_val.as_mapping().ok_or_else(|| {
        CoreError::Recipe(format!("recipe '{recipe_name}': 'params' must be a mapping"))
    })?;

    for (key, val) in params_map {
        let param_name = key.as_str().ok_or_else(|| {
            CoreError::Recipe(format!(
                "recipe '{recipe_name}': param key must be a string"
            ))
        })?;

        let param_map = val.as_mapping().ok_or_else(|| {
            CoreError::Recipe(format!(
                "recipe '{recipe_name}': param '{param_name}' must be a mapping"
            ))
        })?;

        let param_type = match get_string(param_map, "type").as_deref() {
            Some("string") | None => ParamType::String,
            Some("int") => ParamType::Int,
            Some(t) => {
                return Err(CoreError::Recipe(format!(
                    "recipe '{recipe_name}': param '{param_name}' has unknown type '{t}'"
                )));
            }
        };

        let required = param_map
            .get(Value::String("required".into()))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let default = get_string(param_map, "default");
        let description = get_string(param_map, "description");

        let enum_values = param_map
            .get(Value::String("enum".into()))
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            });

        params.insert(
            param_name.to_string(),
            ParamDef {
                param_type,
                required,
                default,
                description,
                enum_values,
            },
        );
    }

    Ok(params)
}

fn parse_lookups(
    mapping: &Mapping,
    recipe_name: &str,
) -> Result<HashMap<String, HashMap<String, String>>> {
    let mut lookups = HashMap::new();

    let lookups_val = match mapping.get(Value::String("lookups".into())) {
        Some(v) => v,
        None => return Ok(lookups),
    };

    let lookups_map = lookups_val.as_mapping().ok_or_else(|| {
        CoreError::Recipe(format!(
            "recipe '{recipe_name}': 'lookups' must be a mapping"
        ))
    })?;

    for (key, val) in lookups_map {
        let param_name = key.as_str().ok_or_else(|| {
            CoreError::Recipe(format!(
                "recipe '{recipe_name}': lookup key must be a string"
            ))
        })?;

        let val_map = val.as_mapping().ok_or_else(|| {
            CoreError::Recipe(format!(
                "recipe '{recipe_name}': lookup '{param_name}' must be a mapping"
            ))
        })?;

        let mut inner = HashMap::new();
        for (k, v) in val_map {
            let k_str = k.as_str().ok_or_else(|| {
                CoreError::Recipe(format!(
                    "recipe '{recipe_name}': lookup '{param_name}' key must be a string"
                ))
            })?;
            let v_str = v.as_str().ok_or_else(|| {
                CoreError::Recipe(format!(
                    "recipe '{recipe_name}': lookup '{param_name}.{k_str}' value must be a string"
                ))
            })?;
            inner.insert(k_str.to_string(), v_str.to_string());
        }

        lookups.insert(param_name.to_string(), inner);
    }

    Ok(lookups)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_recipe_yaml() -> &'static str {
        r#"
name: test-recipe
description: "A test recipe"
params:
  owner:
    type: string
    required: true
    description: "The owner"
  mode:
    type: string
    required: false
    default: create_if_missing
    description: "File mode"
    enum:
      - create_if_missing
      - exact_match
rules:
  - !ensure_file
    path: CODEOWNERS
    content: "* {{ owner }}"
    mode: "{{ mode }}"
"#
    }

    fn lookup_recipe_yaml() -> &'static str {
        r#"
name: lookup-recipe
description: "A recipe with lookups"
params:
  template:
    type: string
    required: true
    description: "Template name"
    enum:
      - alpha
      - beta
  extra:
    type: string
    required: false
    default: ""
    description: "Extra info"
lookups:
  template:
    alpha: "alpha-content {{ extra }}"
    beta: "beta-content"
rules:
  - !ensure_file
    path: "output.txt"
    content: "{{ template }}"
"#
    }

    #[test]
    fn parse_recipe() {
        let recipe = Recipe::from_yaml(simple_recipe_yaml()).unwrap();
        assert_eq!(recipe.name, "test-recipe");
        assert_eq!(recipe.description, "A test recipe");
        assert_eq!(recipe.params.len(), 2);

        let owner = &recipe.params["owner"];
        assert_eq!(owner.param_type, ParamType::String);
        assert!(owner.required);
        assert!(owner.default.is_none());

        let mode = &recipe.params["mode"];
        assert!(!mode.required);
        assert_eq!(mode.default.as_deref(), Some("create_if_missing"));
        assert_eq!(
            mode.enum_values.as_ref().unwrap(),
            &["create_if_missing", "exact_match"]
        );
    }

    #[test]
    fn expand_with_valid_params() {
        let recipe = Recipe::from_yaml(simple_recipe_yaml()).unwrap();
        let mut params = HashMap::new();
        params.insert("owner".to_string(), "@my-team".to_string());

        let expanded = recipe.expand(&params).unwrap();
        let map = expanded.as_mapping().unwrap();

        assert_eq!(
            map.get(Value::String("name".into())),
            Some(&Value::String("test-recipe".into()))
        );

        let rules = map
            .get(Value::String("rules".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn expand_missing_required_param() {
        let recipe = Recipe::from_yaml(simple_recipe_yaml()).unwrap();
        let params = HashMap::new();

        let err = recipe.expand(&params).unwrap_err();
        assert!(err.to_string().contains("missing required parameter 'owner'"));
    }

    #[test]
    fn expand_unknown_param() {
        let recipe = Recipe::from_yaml(simple_recipe_yaml()).unwrap();
        let mut params = HashMap::new();
        params.insert("owner".to_string(), "@team".to_string());
        params.insert("bogus".to_string(), "value".to_string());

        let err = recipe.expand(&params).unwrap_err();
        assert!(err.to_string().contains("unknown parameter 'bogus'"));
    }

    #[test]
    fn expand_invalid_enum_value() {
        let recipe = Recipe::from_yaml(simple_recipe_yaml()).unwrap();
        let mut params = HashMap::new();
        params.insert("owner".to_string(), "@team".to_string());
        params.insert("mode".to_string(), "invalid_mode".to_string());

        let err = recipe.expand(&params).unwrap_err();
        assert!(err.to_string().contains("not in allowed values"));
    }

    #[test]
    fn expand_uses_defaults() {
        let recipe = Recipe::from_yaml(simple_recipe_yaml()).unwrap();
        let mut params = HashMap::new();
        params.insert("owner".to_string(), "@team".to_string());
        // mode not provided, should use default "create_if_missing"

        let expanded = recipe.expand(&params).unwrap();
        let map = expanded.as_mapping().unwrap();

        let rules = map
            .get(Value::String("rules".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn expand_with_lookups() {
        let recipe = Recipe::from_yaml(lookup_recipe_yaml()).unwrap();
        let mut params = HashMap::new();
        params.insert("template".to_string(), "alpha".to_string());
        params.insert("extra".to_string(), "bonus".to_string());

        let expanded = recipe.expand(&params).unwrap();
        let map = expanded.as_mapping().unwrap();

        let rules = map
            .get(Value::String("rules".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn template_substitute_simple() {
        let mut params = HashMap::new();
        params.insert("name".to_string(), "hello".to_string());

        let result = template_substitute("value: {{ name }}", &params);
        assert_eq!(result, "value: hello");
    }

    #[test]
    fn template_substitute_no_spaces() {
        let mut params = HashMap::new();
        params.insert("name".to_string(), "hello".to_string());

        let result = template_substitute("value: {{name}}", &params);
        assert_eq!(result, "value: hello");
    }

    #[test]
    fn template_substitute_multiline() {
        let mut params = HashMap::new();
        params.insert("content".to_string(), "line1\nline2\nline3".to_string());

        let template = "    data: |\n      {{ content }}";
        let result = template_substitute(template, &params);
        assert_eq!(result, "    data: |\n      line1\n      line2\n      line3");
    }

    #[test]
    fn template_substitute_unknown_key_left_as_is() {
        let params = HashMap::new();
        let result = template_substitute("value: {{ unknown }}", &params);
        assert_eq!(result, "value: ");
    }

    #[test]
    fn parse_all_builtins() {
        let recipes = builtin::builtin_recipes().unwrap();
        assert_eq!(recipes.len(), 6);

        let names: Vec<&str> = recipes.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"codeowners"));
        assert!(names.contains(&"security-md"));
        assert!(names.contains(&"license"));
        assert!(names.contains(&"editorconfig"));
        assert!(names.contains(&"gitignore"));
        assert!(names.contains(&"dependabot"));
    }

    #[test]
    fn expand_codeowners_recipe() {
        let recipes = builtin::builtin_recipes().unwrap();
        let recipe = recipes.iter().find(|r| r.name == "codeowners").unwrap();

        let mut params = HashMap::new();
        params.insert("default_owner".to_string(), "@my-org/platform".to_string());

        let expanded = recipe.expand(&params).unwrap();
        let map = expanded.as_mapping().unwrap();
        let rules = map
            .get(Value::String("rules".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn expand_dependabot_recipe() {
        let recipes = builtin::builtin_recipes().unwrap();
        let recipe = recipes.iter().find(|r| r.name == "dependabot").unwrap();

        let mut params = HashMap::new();
        params.insert("ecosystem".to_string(), "cargo".to_string());

        let expanded = recipe.expand(&params).unwrap();
        let map = expanded.as_mapping().unwrap();
        let rules = map
            .get(Value::String("rules".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn expand_gitignore_recipe() {
        let recipes = builtin::builtin_recipes().unwrap();
        let recipe = recipes.iter().find(|r| r.name == "gitignore").unwrap();

        let mut params = HashMap::new();
        params.insert("template".to_string(), "rust".to_string());

        let expanded = recipe.expand(&params).unwrap();
        let map = expanded.as_mapping().unwrap();
        let rules = map
            .get(Value::String("rules".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn expand_license_with_author() {
        let recipes = builtin::builtin_recipes().unwrap();
        let recipe = recipes.iter().find(|r| r.name == "license").unwrap();

        let mut params = HashMap::new();
        params.insert("license_type".to_string(), "MIT".to_string());
        params.insert("author".to_string(), "Acme Corp".to_string());

        let expanded = recipe.expand(&params).unwrap();
        let map = expanded.as_mapping().unwrap();
        let rules = map
            .get(Value::String("rules".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(rules.len(), 1);
    }
}
