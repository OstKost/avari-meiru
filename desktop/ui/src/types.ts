/**
 * Типы данных клиента Mieru по docs/core-contract.md и configs/profile.schema.json
 */

export type TransportType = 'TCP' | 'UDP';

export interface MieruProfile {
  server: string;
  server_port: number;
  transport: TransportType;
  username: string;
  password?: string;
  traffic_pattern?: string;
}

export type VpnState =
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'disconnecting'
  | 'error';

export interface VpnStatus {
  state: VpnState;
  message: string;
  since: string; // RFC3339 timestamp
}

export type ServerHealthState = 'ready' | 'error' | 'unconfigured' | 'busy';

export interface ServerHealth {
  state: ServerHealthState;
  latency_ms: number | null;
  message: string;
  checked_at: string; // RFC3339 timestamp
}

export type SphereVisualState =
  | 'cold_start'
  | 'unconfigured'
  | 'ready'
  | 'connecting'
  | 'connected'
  | 'error'
  | 'disconnecting';

export interface LogEntry {
  id: string;
  timestamp: Date;
  level: 'info' | 'warn' | 'error';
  message: string;
}

export type ThemeMode = 'light' | 'dark' | 'system';
