export type Metrics = Record<string, number | null>;
export type Point = {
  user_id: number;
  x: number;
  y: number;
  group: number;
  noise: boolean;
};
export type Row = Record<string, unknown>;
export type Page = { rows: Row[]; total: number; offset: number };
export type AnalysisResult = {
  task: "classification" | "clustering" | "reduction";
  algorithm: string;
  version: string;
  data_version: string;
  rule_version: string;
  split: {
    train_end: number;
    validation_end: number;
    counts: Record<string, number>;
  };
  parameters: Record<string, unknown>;
  limitations: string[];
  target?: {
    user_id: number;
    movie_id: number;
    movie_title: string;
    probability: number;
    positive: boolean;
    cold_start: boolean;
  };
  metrics: Metrics & { validation?: Metrics; test?: Metrics };
  metric_note?: string;
  metric_notes?: { validation: string | null; test: string | null };
  temporal_metrics?: { validation: Metrics; test: Metrics; selection: string };
  memberships?: {
    user_id: number;
    group: number | null;
    noise: boolean;
    status: string;
  }[];
  groups?: {
    group: number;
    size: number;
    genres: { name: string; share: number }[];
    movies: { movie_id: number; title: string }[];
  }[];
  original_dimensions?: number;
  dimensions?: number;
  information?: { retained: number; relative_error: number };
  comparison?: ({ space: string } & Metrics)[];
  comparison_notes?: (string | null)[];
  reference_metrics?: {
    original: Metrics;
    reduced: Metrics;
    agreement: number | null;
    note: string | null;
  };
  resources?: Record<string, number>;
  axes?: string[];
};
