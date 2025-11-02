export interface ScopeDescription {
  key: string;
  description: string;
  name: string;
  parent?: string;
}
export interface SmallIdentification {
  name: string;
}
export interface UserResponseType {
  id: number;
  name: string;
  username: string;
  email: string;
  admin: boolean;
  user_manager: boolean;
  system_manager: boolean;
  default_repository_actions: Array<RepositoryActions>;
  created_at: string;
}

export interface PublicUser {
  id: number;
  name: string;
  username: string;
  admin: boolean;
}

export interface Session {
  user_id: number;
  session_id: string;
  expires: Date;
  created: Date;
}

export interface Me {
  user: UserResponseType;
  session: Session;
}

export interface NewUser {
  username: string;
  password: string;
  email: string;
  name: string;
}
export interface SiteInfo {
  url?: string;
  mode: string;
  name: string;
  description: string;
  is_installed: boolean;
  version: string;
  password_rules?: PasswordRules;
  sso?: SsoInfo;
}
export interface PasswordRules {
  min_length: number;
  require_uppercase: boolean;
  require_lowercase: boolean;
  require_number: boolean;
  require_symbol: boolean;
  require_special?: boolean;
}

export interface SsoInfo {
  login_path: string;
  login_button_text: string;
  provider_login_url?: string | null;
  provider_redirect_param?: string | null;
  auto_create_users: boolean;
}

export interface SsoConfiguration extends SsoInfo {
  enabled: boolean;
  username_header: string;
  email_header?: string | null;
  display_name_header?: string | null;
}

export enum RepositoryActions {
  Read = "Read",
  Write = "Write",
  Edit = "Edit",
}
export function formatDate(date: Date) {
  return `${date.getFullYear()}-${date.getMonth() + 1}-${date.getDate()}`;
}
