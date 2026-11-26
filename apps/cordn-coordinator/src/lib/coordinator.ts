import { invoke } from '@tauri-apps/api/core';

const plugin = 'plugin:cordn-android-service';

export type CoordinatorConfig = {
	relayUrls: string[];
	serverName: string;
	serverAbout: string | null;
	serverWebsite: string | null;
	isAnnounced: boolean;
	maxAgeDays: number;
};

export type ServiceDiagnostics = {
	logs: string;
	storageBytes: number;
	startedAt: number | null;
	keyPackages: number | null;
	welcomes: number | null;
	joinRequests: number | null;
	groupMessages: number | null;
};

export type ServiceStatus = {
	healthLabel?: string;
	health?: { updatedAt: number; startedAt: number; loopAgeMs: number; connectedRelays: number; totalRelays: number; rejectedSubscriptions: number; subscriptionRepairs: number; lastRequestAt?: number; lastRelayAckAt?: number; criticalTaskFailed: boolean };
	state: 'stopped' | 'starting' | 'running' | 'stopping' | 'error';
	publicKey?: string;
	lastError?: string;
	recoveryAttempt?: number;
	nextRetryAt?: number;
	batteryExempt: boolean;
};

export type PubkeyDisplay = { npub: string; nprofile: string };

export type InstanceInfo = { id: string; name: string; createdAt: number };
export type InstanceStatus = { instance: InstanceInfo; status: ServiceStatus; config: CoordinatorConfig | null };
export type InstancesResponse = { instances: InstanceStatus[] };

export const MAX_INSTANCES = 5;

export const defaultConfig: CoordinatorConfig = {
	relayUrls: ['wss://relay.contextvm.org', 'wss://relay2.contextvm.org', 'wss://cordn.feeds.relay.tools'],
	serverName: 'cordn-server',
	serverAbout: null,
	serverWebsite: null,
	isAnnounced: false,
	maxAgeDays: 30
};

export function listInstances(): Promise<InstancesResponse> {
	return invoke(`${plugin}|list_instances`);
}
export function createInstance(name?: string): Promise<InstanceInfo> {
	return invoke(`${plugin}|create_instance`, { payload: { name: name || null } });
}
export function renameInstance(instanceId: string, name: string): Promise<InstanceInfo> {
	return invoke(`${plugin}|rename_instance`, { payload: { instanceId, name } });
}
export function deleteInstance(instanceId: string): Promise<InstancesResponse> {
	return invoke(`${plugin}|delete_instance`, { payload: { instanceId } });
}

export function startInstance(instanceId: string, config: CoordinatorConfig, privateKeyHex?: string): Promise<ServiceStatus> {
	return invoke(`${plugin}|start_instance`, { payload: { instanceId, config, privateKeyHex: privateKeyHex || null } });
}
export function stopInstance(instanceId: string): Promise<ServiceStatus> {
	return invoke(`${plugin}|stop_instance`, { payload: { instanceId } });
}
export function getInstanceStatus(instanceId: string): Promise<ServiceStatus> {
	return invoke(`${plugin}|status_instance`, { payload: { instanceId } });
}
export function getInstanceDiagnostics(instanceId: string): Promise<ServiceDiagnostics> {
	return invoke(`${plugin}|diagnostics_instance`, { payload: { instanceId } });
}
export function getInstanceConfig(instanceId: string): Promise<{ config: CoordinatorConfig; saved: boolean }> {
	return invoke<{ config: CoordinatorConfig | null }>(`${plugin}|config_instance`, { payload: { instanceId } })
		.then((r) => ({ config: r.config ?? defaultConfig, saved: r.config !== null }));
}
export function flushDataInstance(instanceId: string): Promise<ServiceStatus> {
	return invoke(`${plugin}|flush_data_instance`, { payload: { instanceId } });
}
export function resetIdentityInstance(instanceId: string): Promise<ServiceStatus> {
	return invoke(`${plugin}|reset_identity_instance`, { payload: { instanceId } });
}
export function publishRelayListInstance(instanceId: string): Promise<PublishRelayListResult> {
	return invoke(`${plugin}|publish_relay_list_instance`, { payload: { instanceId } });
}
export function restartScheduleInstance(instanceId: string, operation: 'get' | 'save' | 'permission' = 'get', enabled = false, times = ['03:00']): Promise<RestartSchedule> {
	return invoke(`${plugin}|restart_schedule_instance`, { instanceId, payload: { operation, enabled, times } });
}

// Legacy compat (default instance) - kept so older UI code compiles during transition
export function start(config: CoordinatorConfig, privateKeyHex?: string): Promise<ServiceStatus> { return invoke(`${plugin}|start`, { payload: { config, privateKeyHex: privateKeyHex || null } }); }
export function stop(): Promise<ServiceStatus> { return invoke(`${plugin}|stop`); }
export function getStatus(): Promise<ServiceStatus> { return invoke(`${plugin}|status`); }
export function getDiagnostics(): Promise<ServiceDiagnostics> { return invoke(`${plugin}|diagnostics`); }
export async function getConfig(): Promise<{ config: CoordinatorConfig; saved: boolean }> { const r = await invoke<{ config: CoordinatorConfig | null }>(`${plugin}|config`); return { config: r.config ?? defaultConfig, saved: r.config !== null }; }
export function encodePubkey(publicKey: string, relayUrls: string[]): Promise<PubkeyDisplay> { return invoke(`${plugin}|encode_pubkey`, { payload: { publicKey, relayUrls } }); }
export function flushData(): Promise<ServiceStatus> { return invoke(`${plugin}|flush_data`); }
export function resetIdentity(): Promise<ServiceStatus> { return invoke(`${plugin}|reset_identity`); }
export function requestBatteryExemption(): Promise<void> { return invoke(`${plugin}|request_battery_exemption`); }
export function requestNotificationPermission(): Promise<void> { return invoke(`${plugin}|request_notification_permission`); }
export type RestartSchedule = { enabled: boolean; times: string[]; exactAlarmAllowed: boolean; nextAlarm: number | null };
export function restartSchedule(operation: 'get' | 'save' | 'permission' = 'get', enabled = false, times = ['03:00']): Promise<RestartSchedule> { return invoke(`${plugin}|restart_schedule`, { payload: { operation, enabled, times } }); }
export type PublishRelayListResult = { sent: number; failed: number; targets: string[] };
export function publishRelayList(): Promise<PublishRelayListResult> { return invoke(`${plugin}|publish_relay_list`); }
