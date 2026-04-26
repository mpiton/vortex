export type PluginConfigFieldType =
  | "string"
  | "boolean"
  | "integer"
  | "float"
  | "url"
  | "enum"
  | "array";

export interface PluginConfigField {
  key: string;
  fieldType: PluginConfigFieldType;
  default: string | null;
  description: string | null;
  options: string[];
  min: number | null;
  max: number | null;
  regex: string | null;
}

export interface PluginConfigView {
  fields: PluginConfigField[];
  values: Record<string, string>;
}
