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
	state: 'stopped' | 'starting' | 'running' | 'stopping' | 'error';
	publicKey?: string;
	lastError?: string;
	batteryExempt: boolean;
};

export type PubkeyDisplay = {
	npub: string;
	nprofile: string;
};

export const defaultConfig: CoordinatorConfig = {
	relayUrls: ['wss://relay.contextvm.org', 'wss://relay.nostr.net'],
	serverName: 'cordn-server',
	serverAbout: null,
	serverWebsite: null,
	isAnnounced: false,
	maxAgeDays: 30
};

export function start(config: CoordinatorConfig, privateKeyHex?: string): Promise<ServiceStatus> {
	return invoke(`${plugin}|start`, {
		payload: { config, privateKeyHex: privateKeyHex || null }
	});
}

export function stop(): Promise<ServiceStatus> {
	return invoke(`${plugin}|stop`);
}

export function getStatus(): Promise<ServiceStatus> {
	return invoke(`${plugin}|status`);
}

export function getDiagnostics(): Promise<ServiceDiagnostics> {
	return invoke(`${plugin}|diagnostics`);
}

export function encodePubkey(publicKey: string, relayUrls: string[]): Promise<PubkeyDisplay> {
	return invoke(`${plugin}|encode_pubkey`, {
		payload: { publicKey, relayUrls }
	});
}

export function flushData(): Promise<ServiceStatus> {
	return invoke(`${plugin}|flush_data`);
}

export function resetIdentity(): Promise<ServiceStatus> {
	return invoke(`${plugin}|reset_identity`);
}

export async function getConfig(): Promise<CoordinatorConfig> {
	const response = await invoke<{ config: CoordinatorConfig | null }>(`${plugin}|config`);
	return response.config ?? defaultConfig;
}

export function requestBatteryExemption(): Promise<void> {
	return invoke(`${plugin}|request_battery_exemption`);
}

export function requestNotificationPermission(): Promise<void> {
	return invoke(`${plugin}|request_notification_permission`);
}

export type RestartSchedule = {
    enabled: boolean;
    times: string[];
    exactAlarmAllowed: boolean;
    nextAlarm: number | null;
};

export function restartSchedule(operation: 'get' | 'save' | 'permission' = 'get', enabled = false, times = ['03:00']): Promise<RestartSchedule> {
    return invoke(`${plugin}|restart_schedule`, { payload: { operation, enabled, times } });
}
