export type RuleEntry = {
  enabled: boolean;
  value: number | { min: number; max: number } | null;
};
export type Rules = { entries: Record<string, RuleEntry> };
export type RuleSpec = {
  id: string;
  category: string;
  label: string;
  description: string;
  required: boolean;
  default_enabled: boolean;
  input: "toggle" | "integer" | "range";
  default_value?: unknown;
  min?: number;
  max?: number;
};
export type Settings = {
  api_key: string;
  api_base: string;
  model: string;
  hadoop_mode: string;
  hadoop_bin: string;
  hadoop_conf_dir: string;
  streaming_jar: string;
  python_bin: string;
  hdfs_root: string;
  ssh_target: string;
  remote_hadoop_bin: string;
  remote_python_bin: string;
  rules: Rules;
};
export type Message = {
  role: string;
  content: string;
  created_at: string;
  run_id: string | null;
  kind: string;
  job_id: string | null;
  tool_name: string | null;
  rule_id: string | null;
};
export type Conversation = {
  id: string;
  title: string;
  messages: Message[];
  job_ids: string[];
  updated_at: string;
  rules: Rules | null;
  rules_revision: number;
};
export type UploadedFile = {
  id: string;
  name: string;
  sha256: string;
  bytes: number;
  hdfs_path: string | null;
};
export type Job = {
  id: string;
  display_number: number;
  conversation_id: string;
  kind: string;
  parent_id: string | null;
  run_id: string | null;
  status: string;
  stage: string;
  error: string | null;
  result: any | null;
};
export type Database = {
  settings: Settings;
  conversations: Conversation[];
  files: UploadedFile[];
  active_files: Record<string, string>;
  jobs: Job[];
  source_revision: number;
};
export type SourceStatus = Record<
  string,
  { exists: boolean; file: UploadedFile | null; hdfs_path: string | null }
>;
export type AgentEvent = {
  conversation_id: string;
  run_id: string;
  kind: string;
  value: any;
};
