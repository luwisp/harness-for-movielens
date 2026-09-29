import { invoke } from "@tauri-apps/api/core";
import type {
  Database,
  Job,
  RuleSpec,
  Rules,
  Settings,
  SourceStatus,
} from "./types";
export const api = {
  state: () => invoke<Database>("get_state"),
  catalog: () => invoke<RuleSpec[]>("rule_catalog"),
  sourceStatus: () => invoke<SourceStatus>("source_status"),
  createConversation: () => invoke<{ id: string }>("create_conversation"),
  rename: (id: string, title: string) =>
    invoke("rename_conversation", { id, title }),
  deleteConversation: (id: string) =>
    invoke<void>("delete_conversation", { id }),
  saveSettings: (settings: Settings) => invoke("save_settings", { settings }),
  updateRules: (conversationId: string, rules: Rules) =>
    invoke("update_rules", { conversationId, rules }),
  start: (conversationId: string, content: string) =>
    invoke<string>("start_chat", { conversationId, content }),
  stop: (conversationId: string) => invoke("stop_chat", { conversationId }),
  preview: (job: Job, name: string) =>
    invoke<string[]>("preview_result", { jobId: job.id, name }),
  download: (job: Job, name: string) =>
    invoke<string>("download_result", { jobId: job.id, name }),
  records: (
    job: Job,
    offset = 0,
    limit = 20,
    ruleId?: string,
    table?: string,
    action?: string,
  ) =>
    invoke<{ total: number; records: any[] }>("cleaning_records", {
      jobId: job.id,
      offset,
      limit,
      ruleId,
      table,
      action,
    }),
  async upload(file: File, progress: (p: number) => void) {
    const id = await invoke<string>("begin_upload", {
      name: file.name,
      size: file.size,
    });
    try {
      for (let offset = 0; offset < file.size; offset += 1024 * 1024) {
        const bytes = Array.from(
          new Uint8Array(
            await file.slice(offset, offset + 1024 * 1024).arrayBuffer(),
          ),
        );
        await invoke("append_upload_chunk", { id, bytes });
        progress(
          Math.min(
            100,
            Math.round((100 * (offset + bytes.length)) / file.size),
          ),
        );
      }
      return await invoke<unknown>("finish_upload", { id });
    } catch (error) {
      await invoke("abort_upload", { id }).catch(() => {});
      throw error;
    }
  },
};
