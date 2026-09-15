export interface QuickAction {
  id: string;
  project_id: string;
  name: string;
  description: string;
  workflow_id: string;
  enabled: boolean;
}

export interface SaveQuickActionRequest {
  id: string | null;
  project_id: string;
  name: string;
  description: string;
  workflow_id: string;
  enabled: boolean;
}

export interface SpecialistTemplate {
  id: string;
  project_id: string;
  name: string;
  description: string;
  instructions: string;
  enabled: boolean;
}

export interface SaveSpecialistTemplateRequest {
  id: string | null;
  project_id: string;
  name: string;
  description: string;
  instructions: string;
  enabled: boolean;
}
