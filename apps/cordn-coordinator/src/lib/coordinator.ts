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
	nprofile?: string;
	lastError?: string;
	batteryExempt: boolean;
};

export const defaultConfig: CoordinatorConfig = {
	relayUrls: ['wss://relay.contextvm.org'],
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
