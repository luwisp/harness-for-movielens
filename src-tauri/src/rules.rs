use crate::model::{RuleEntry, Rules};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize)]
pub struct RuleSpec {
    pub id: String, pub category: String, pub label: String, pub description: String,
    pub required: bool, pub default_enabled: bool, pub input: String,
    pub default_value: Option<Value>, pub min: Option<i64>, pub max: Option<i64>,
}
pub fn catalog() -> Vec<RuleSpec> {
    serde_json::from_str(include_str!("../resources/rule_catalog.json")).expect("valid rule catalog")
}
pub fn describe(id: &str) -> (String, String) {
    catalog().into_iter().find(|spec| spec.id == id)
        .map(|spec| (spec.label, spec.description))
        .unwrap_or_else(|| (id.to_string(), "历史规则，当前目录无说明".into()))
}
pub fn default_rules() -> Rules {
    Rules { entries: catalog().into_iter().map(|s| (s.id, RuleEntry {
        enabled: s.default_enabled, value: s.default_value,
    })).collect::<BTreeMap<_,_>>() }
}
pub fn normalize(rules: &mut Rules) {
    // v1 relative freshness cannot be converted to a fixed historical window.
    if let Some(old) = rules.entries.remove("score_freshness") {
        rules.entries.entry("score_up_to_date".into()).or_insert(RuleEntry {
            enabled: old.enabled, value: None,
        });
    }
    rules.entries.remove("score_reference");
    for spec in catalog() { rules.entries.entry(spec.id).or_insert(RuleEntry {
        enabled: spec.default_enabled, value: spec.default_value,
    }); }
}
pub fn validate(rules: &Rules) -> Result<(), String> {
    let specs = catalog();
    for (id, entry) in &rules.entries {
        let spec = specs.iter().find(|s| &s.id == id).ok_or_else(|| format!("未知规则: {id}"))?;
        if spec.required && !entry.enabled { return Err(format!("{} 是必要规则", spec.label)); }
        match spec.input.as_str() {
            "integer" => { let n = entry.value.as_ref().and_then(Value::as_i64).ok_or_else(|| format!("{} 参数无效", spec.label))?;
                if n < spec.min.unwrap_or(i64::MIN) || n > spec.max.unwrap_or(i64::MAX) { return Err(format!("{} 超出范围", spec.label)); } },
            "range" => { let value = entry.value.as_ref().ok_or_else(|| format!("{} 缺少区间", spec.label))?;
                let a = value["min"].as_i64().ok_or_else(|| format!("{} 最小值无效", spec.label))?;
                let b = value["max"].as_i64().ok_or_else(|| format!("{} 最大值无效", spec.label))?;
                if a > b || a < spec.min.unwrap_or(i64::MIN) || b > spec.max.unwrap_or(i64::MAX) { return Err(format!("{} 区间无效", spec.label)); } },
            "toggle" => {}, _ => return Err(format!("未知规则类型: {}", spec.input)),
        }
    }
    for spec in specs { if !rules.entries.contains_key(&spec.id) { return Err(format!("缺少规则: {}", spec.id)); } }
    Ok(())
}
pub fn update(rules: &mut Rules, id: &str, enabled: Option<bool>, value: Option<Value>) -> Result<(), String> {
    let spec = catalog().into_iter().find(|s| s.id == id).ok_or("规则不存在")?;
    let entry = rules.entries.get_mut(id).ok_or("规则不存在")?;
    if let Some(flag) = enabled { entry.enabled = flag; }
    if let Some(value) = value {
        if spec.input == "toggle" { return Err("此规则没有数值参数".into()); }
        entry.value = Some(value);
    }
    validate(rules)
}
pub fn version(rules: &Rules) -> String {
    let bytes = serde_json::to_vec(rules).unwrap_or_default();
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_rules_can_change_independently() {
        let mut rules = default_rules();
        update(&mut rules, "unique_users", Some(false), None).unwrap();
        assert!(!rules.entries["unique_users"].enabled);
        assert!(rules.entries["unique_movies"].enabled);
        assert!(update(&mut rules, "schema_fields", Some(false), None).is_err());
    }
    #[test]
    fn range_checks_are_enforced() {
        let mut rules = default_rules();
        assert!(update(&mut rules, "rating_range", None, Some(serde_json::json!({"min":5,"max":1}))).is_err());
    }
}
