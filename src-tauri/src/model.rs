use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleEntry { pub enabled: bool, pub value: Option<Value> }

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules { pub entries: BTreeMap<String, RuleEntry> }
impl Default for Rules { fn default() -> Self { crate::rules::default_rules() } }

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub api_key: String, pub api_base: String, pub model: String,
    pub hadoop_mode: String, pub hadoop_bin: String, pub hadoop_conf_dir: String,
    pub streaming_jar: String, pub python_bin: String, pub hdfs_root: String,
    pub ssh_target: String, pub remote_hadoop_bin: String, pub remote_python_bin: String,
    pub rules: Rules,
}
impl Default for Settings {
    fn default() -> Self {
        Self { api_key: String::new(), api_base: "https://api.deepseek.com".into(), model: "deepseek-flash".into(),
            hadoop_mode: "local".into(), hadoop_bin: if cfg!(windows) { "hadoop.cmd" } else { "hadoop" }.into(),
            hadoop_conf_dir: String::new(), streaming_jar: String::new(),
            python_bin: if cfg!(windows) { "python" } else { "python3" }.into(),
            hdfs_root: "/ml1m-agent".into(), ssh_target: String::new(), remote_hadoop_bin: "hadoop".into(),
            remote_python_bin: "python3".into(), rules: Rules::default() }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Message {
    pub role: String, pub content: String, pub created_at: String,
    pub run_id: Option<String>, pub kind: String, pub job_id: Option<String>,
    pub tool_name: Option<String>, pub rule_id: Option<String>,
}
impl Default for Message {
    fn default() -> Self { Self { role: String::new(), content: String::new(), created_at: now(),
        run_id: None, kind: "text".into(), job_id: None, tool_name: None, rule_id: None } }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Conversation {
    pub id: String, pub title: String, pub messages: Vec<Message>,
    pub file_ids: Vec<String>, pub job_ids: Vec<String>, pub updated_at: String,
    pub rules: Option<Rules>, pub rules_revision: u64,
}
impl Default for Conversation {
    fn default() -> Self { Self { id: String::new(), title: "新对话".into(), messages: vec![],
        file_ids: vec![], job_ids: vec![], updated_at: now(), rules: None, rules_revision: 1 } }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct UploadedFile {
    pub id: String, pub conversation_id: String, pub name: String,
    pub sha256: String, pub bytes: u64, pub local_path: String,
    pub hdfs_path: Option<String>, pub uploaded_at: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Job {
    pub id: String, pub conversation_id: String, pub status: String,
    pub stage: String, pub error: Option<String>, pub result: Option<Value>,
    pub created_at: String, pub updated_at: String,
    pub kind: String, pub parent_id: Option<String>, pub run_id: Option<String>,
    pub display_number: u64,
}
impl Default for Job {
    fn default() -> Self { Self { id: String::new(), conversation_id: String::new(), status: "queued".into(),
        stage: String::new(), error: None, result: None, created_at: now(), updated_at: now(),
        kind: "cleaning".into(), parent_id: None, run_id: None, display_number: 0 } }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Database {
    pub settings: Settings, pub conversations: Vec<Conversation>,
    pub files: Vec<UploadedFile>, pub active_files: BTreeMap<String, String>, pub jobs: Vec<Job>,
    pub source_revision: u64,
}

pub fn now() -> String { chrono::Utc::now().to_rfc3339() }
