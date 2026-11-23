export interface Setting {
  key: string;
  /** JSON-encoded value */
  value: string;
  updated_at: string;
}

export interface ActivityEntry {
  id: string;
  entity_type: string;
  entity_id: string;
  action: string;
  actor: "user" | "ai" | "automation";
  before_json: string | null;
  after_json: string | null;
  batch_id: string | null;
  created_at: string;
}

export interface AppError {
  kind: "validation" | "not_found" | "storage";
  message: string;
}
