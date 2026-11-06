<script lang="ts">
	import { onMount, tick } from 'svelte';
	import QRCode from 'qrcode';
	import { openUrl } from '@tauri-apps/plugin-opener';
	import {
		defaultConfig,
		encodePubkey,
		flushData,
		getConfig,
		getDiagnostics,
		getStatus,
		restartSchedule,
		type RestartSchedule,
		requestBatteryExemption,
		requestNotificationPermission,
		resetIdentity,
		start,
		stop,
		type PubkeyDisplay,
		type ServiceDiagnostics,
		type ServiceStatus
	} from '$lib/coordinator';

	let status = $state<ServiceStatus>({ state: 'stopped', batteryExempt: false });
    let schedule = $state<RestartSchedule | null>(null);
    let scheduleEnabled = $state(false);
    let scheduleCount = $state('1');
    let firstRestart = $state('03:00');
    let secondRestart = $state('15:00');
    let scheduleBusy = $state(false);
    let scheduleError = $state('');
    let scheduleSaved = $state(false);

    async function refreshSchedule() {
        try { schedule = await restartSchedule(); }
        catch (cause) { scheduleError = message(cause); }
    }

    async function saveSchedule(event: SubmitEvent) {
        event.preventDefault();
        scheduleBusy = true;
        scheduleError = '';
        scheduleSaved = false;
        try {
            schedule = await restartSchedule('save', scheduleEnabled,
                scheduleCount === '2' ? [firstRestart, secondRestart] : [firstRestart]);
            scheduleSaved = true;
        } catch (cause) { scheduleError = message(cause); }
        finally { scheduleBusy = false; }
    }

    async function allowAlarms() {
        try { await restartSchedule('permission'); }
        catch (cause) { scheduleError = message(cause); }
    }

	let diagnostics = $state<ServiceDiagnostics | null>(null);
	let diagnosticsOpen = $state(false);
	let diagnosticsError = $state('');
	let logView = $state<HTMLPreElement>();
	let relays = $state(defaultConfig.relayUrls.join('\n'));
	let serverName = $state(defaultConfig.serverName);
	let serverAbout = $state('');
	let serverWebsite = $state('');
	let isAnnounced = $state(false);
	let maxAgeDays = $state(30);
	let privateKey = $state('');
	let busy = $state(false);
	let error = $state('');
	let pubkeyFormat = $state<'npub' | 'nprofile' | 'hex'>('npub');
	let pubkeyDisplay = $state<PubkeyDisplay | null>(null);
	let qrSvg = $state('');
	let dangerConfirm = $state<'' | 'flush' | 'reset'>('');
	let dangerTimer = 0;
	let statusUnavailable = $state(false);
    let healthLabel = $derived(statusUnavailable ? 'Status unavailable — check connection to service' : status.healthLabel ?? status.state);
	let active = $derived(['starting', 'running', 'stopping'].includes(status.state));
	const relaysList = $derived(
		relays
			.split(/[\n,]/)
			.map((relay) => relay.trim())
			.filter(Boolean)
	);
	let relayCount = $derived(relaysList.length);
	let pubkeyText = $derived(
		pubkeyFormat === 'hex' || !pubkeyDisplay
			? status.publicKey ?? ''
			: pubkeyFormat === 'npub'
				? pubkeyDisplay.npub
				: pubkeyDisplay.nprofile
	);
	let shareUrl = $derived(
		pubkeyDisplay ? `https://cordn.net/chat/coordinators?c=${pubkeyDisplay.nprofile}` : ''
	);

	onMount(() => {
		void load();
		const poll = window.setInterval(() => void refresh(), 2_000);
        const focus = () => { void refreshSchedule(); };
        window.addEventListener('focus', focus);
        const visibility = () => { if (!document.hidden) void refreshSchedule(); };
        document.addEventListener('visibilitychange', visibility);
        return () => {
            window.clearInterval(poll);
            window.removeEventListener('focus', focus);
            document.removeEventListener('visibilitychange', visibility);
        };
	});

	async function load() {
		try {
			const [saved, current] = await Promise.all([getConfig(), getStatus()]);
			relays = saved.relayUrls.join('\n');
			serverName = saved.serverName;
			serverAbout = saved.serverAbout ?? '';
			serverWebsite = saved.serverWebsite ?? '';
			isAnnounced = saved.isAnnounced;
			maxAgeDays = saved.maxAgeDays;
            status = current;
            schedule = await restartSchedule();
            scheduleEnabled = schedule.enabled;
            scheduleCount = String(schedule.times.length);
            firstRestart = schedule.times[0] ?? '03:00';
            secondRestart = schedule.times[1] ?? '15:00';
		} catch (cause) {
			error = message(cause);
		}
	}

	async function refresh() {
		try {
			status = await getStatus();
            statusUnavailable = false;
		} catch {
			statusUnavailable = true;
		}
		if (diagnosticsOpen) await refreshDiagnostics();
	}

	async function refreshDiagnostics() {
		const follow =
			!logView || logView.scrollHeight - logView.scrollTop - logView.clientHeight < 24;
		try {
			diagnostics = await getDiagnostics();
			diagnosticsError = '';
			await tick();
			if (follow && logView) logView.scrollTop = logView.scrollHeight;
		} catch (cause) {
			diagnosticsError = message(cause);
		}
	}

	function toggleDiagnostics(event: Event) {
		diagnosticsOpen = (event.currentTarget as HTMLDetailsElement).open;
		if (diagnosticsOpen) void refreshDiagnostics();
	}

	$effect(() => {
		const key = status.publicKey;
		const relaysNow = relaysList.join('\n');
		if (!key) {
			pubkeyDisplay = null;
			return;
		}
		void encodePubkey(key, relaysNow ? relaysNow.split('\n') : []).then(
			(display) => (pubkeyDisplay = display),
			() => (pubkeyDisplay = null)
		);
	});

	$effect(() => {
		const url = shareUrl;
		if (!url) {
			qrSvg = '';
			return;
		}
		void QRCode.toString(url, { type: 'svg', margin: 0, errorCorrectionLevel: 'M' }).then(
			(svg) => (qrSvg = svg),
			() => (qrSvg = '')
		);
	});

	async function copy(text: string) {
		try {
			await navigator.clipboard.writeText(text);
		} catch (cause) {
			error = message(cause);
		}
	}

	async function shareLink() {
		try {
			await navigator.share({ title: 'Cordn coordinator', url: shareUrl });
		} catch (cause) {
			error = message(cause);
		}
	}

	async function openExternally() {
		try {
			await openUrl(shareUrl);
		} catch (cause) {
			error = message(cause);
		}
	}

	function armDanger(which: 'flush' | 'reset') {
		dangerConfirm = dangerConfirm === which ? '' : which;
		window.clearTimeout(dangerTimer);
		if (dangerConfirm) dangerTimer = window.setTimeout(() => (dangerConfirm = ''), 4_000);
	}

	async function runDestructive(action: () => Promise<ServiceStatus>) {
		dangerConfirm = '';
		busy = true;
		error = '';
		try {
			status = await action();
		} catch (cause) {
			error = message(cause);
		} finally {
			busy = false;
		}
	}

	async function startCoordinator(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			await requestNotificationPermission().catch(() => undefined);
			status = await start(
				{
					relayUrls: relays
						.split(/[\n,]/)
						.map((relay) => relay.trim())
						.filter(Boolean),
					serverName,
					serverAbout: serverAbout.trim() || null,
					serverWebsite: serverWebsite.trim() || null,
					isAnnounced,
					maxAgeDays
				},
				privateKey.trim() || undefined
			);
			privateKey = '';
		} catch (cause) {
			error = message(cause);
		} finally {
			busy = false;
		}
	}

	async function stopCoordinator() {
		busy = true;
		error = '';
		try {
			status = await stop();
		} catch (cause) {
			error = message(cause);
		} finally {
			busy = false;
		}
	}

	async function improveReliability() {
		try {
			await requestBatteryExemption();
		} catch (cause) {
			error = message(cause);
		}
	}

	function formatBytes(bytes: number): string {
		if (bytes < 1024) return `${bytes} B`;
		if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
		return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
	}

	function formatUptime(startedAt: number | null): string {
		if (!startedAt || status.state !== 'running') return '—';
		const seconds = Math.max(0, Math.floor((Date.now() - startedAt) / 1000));
		const days = Math.floor(seconds / 86_400);
		const hours = Math.floor((seconds % 86_400) / 3_600);
		const minutes = Math.floor((seconds % 3_600) / 60);
		if (days) return `${days}d ${hours}h`;
		if (hours) return `${hours}h ${minutes}m`;
		return `${minutes}m ${seconds % 60}s`;
	}

	function count(value: number | null): string {
		return value?.toLocaleString() ?? '—';
	}

	function message(cause: unknown): string {
		return cause instanceof Error ? cause.message : String(cause);
	}
</script>

<svelte:head>
	<title>Cordn Coordinator</title>
	<meta
		name="description"
		content="Run a private cordn MLS delivery coordinator from this Android device"
	/>
</svelte:head>

<main>
	<header>
		<div class="mark" aria-hidden="true"><span></span></div>
		<div>
			<p class="eyebrow">CORDN</p>
			<h1>Coordinator</h1>
		</div>
		<div class:running={status.state === 'running' && !statusUnavailable && healthLabel.startsWith('Relay connected')} class="status-dot" title={status.state}></div>
	</header>

	<section class="status-card" aria-live="polite">
		<div class="status-heading">
			<div>
				<p class="label">Status</p>
				<strong>{healthLabel}</strong>
				<p class="status-copy">
					{status.state === 'running'
						? 'Local transport status only. Relay acknowledgement does not prove client receipt.'
						: status.state === 'starting'
							? 'Starting the background service…'
							: status.state === 'stopping'
								? 'Shutting down safely…'
								: 'Start when you are ready'}
				</p>
			</div>
		</div>

		{#if active}
			<button
				class="action stop"
				type="button"
				onclick={stopCoordinator}
				disabled={busy || status.state === 'stopping'}
			>
				{status.state === 'stopping' ? 'Stopping…' : 'Stop coordinator'}
			</button>
		{:else}
			<button class="action start" type="submit" form="configuration" disabled={busy}>
				{busy ? 'Starting…' : 'Start coordinator'}
			</button>
		{/if}

        {#if status.state === 'running' && status.health}
            <p class="status-copy">Relays: {status.health.connectedRelays}/{status.health.totalRelays} · Rejected subscriptions: {status.health.rejectedSubscriptions} · Retry attempts: {status.health.subscriptionRepairs}</p>
            <p class="status-copy">Last request: {status.health.lastRequestAt ? new Date(status.health.lastRequestAt).toLocaleString() : 'None observed'} · Last relay acknowledgement: {status.health.lastRelayAckAt ? new Date(status.health.lastRelayAckAt).toLocaleString() : 'None observed'}</p>
        {/if}
		{#if status.publicKey}
			<div class="identity">
				<div class="identity-head">
					<p class="label">Coordinator identity</p>
					<div class="format-switch" role="group" aria-label="Public key format">
						{#each ['npub', 'nprofile', 'hex'] as fmt}
							<button
								type="button"
								class:active={pubkeyFormat === fmt}
								onclick={() => (pubkeyFormat = fmt as (typeof pubkeyFormat))}
							>
								{fmt}
							</button>
						{/each}
					</div>
				</div>
				<button type="button" class="copy" onclick={() => copy(pubkeyText)}>
					<code>{pubkeyText}</code>
					<span>Copy</span>
				</button>
			</div>
		{/if}

		{#if !status.batteryExempt}
			<div class="reliability">
				<div>
					<strong>Background use restricted</strong>
					<p>Android may pause relay connections while sleeping.</p>
				</div>
				<button type="button" onclick={improveReliability}>Allow</button>
			</div>
		{/if}
	</section>

	{#if error || status.lastError}
		<p class="error" role="alert">{error || status.lastError}</p>
	{/if}

    <details class="panel schedule-panel">
        <summary>
            <span>
                <strong>Scheduled restarts</strong>
                <small>{schedule?.enabled ? `${schedule.times.join(' & ')} · daily` : 'Off · a fresh connection, on your schedule'}</small>
            </span>
        </summary>
        <form class="panel-content schedule-content" onsubmit={saveSchedule} oninput={() => (scheduleSaved = false)}>
            <label class="toggle"><input type="checkbox" bind:checked={scheduleEnabled} disabled={!schedule || scheduleBusy} />Enable daily restarts</label>
            <p class="schedule-description">Wake the phone to safely restart a running coordinator. A manual Stop is always respected. Your identity and data stay unchanged.</p>
            <fieldset class="schedule-fields" disabled={!scheduleEnabled || scheduleBusy || !schedule}>
                <div class="frequency" role="group" aria-label="Restarts per day">
                    <button type="button" class:selected={scheduleCount === '1'} aria-pressed={scheduleCount === '1'} onclick={() => { scheduleCount = '1'; scheduleSaved = false; }}>Once daily</button>
                    <button type="button" class:selected={scheduleCount === '2'} aria-pressed={scheduleCount === '2'} onclick={() => { scheduleCount = '2'; scheduleSaved = false; }}>Twice daily</button>
                </div>
                <div class="row">
                    <label>First restart<input type="time" bind:value={firstRestart} required /></label>
                    {#if scheduleCount === '2'}<label>Second restart<input type="time" bind:value={secondRestart} required /></label>{/if}
                </div>
                <small>Phone’s local time · follows time-zone changes · keep times 15+ minutes apart</small>
            </fieldset>
            {#if schedule && !schedule.exactAlarmAllowed}
                <div class="reliability alarm-permission">
                    <div><strong>Allow precise wakeups</strong><p>Enable Alarms &amp; reminders in Android, return here, then save.</p></div>
                    <button type="button" onclick={allowAlarms}>Allow</button>
                </div>
            {/if}
            <button class="action start" type="submit" disabled={scheduleBusy || !schedule || (scheduleEnabled && !schedule.exactAlarmAllowed)}>{scheduleBusy ? 'Saving…' : 'Save schedule'}</button>
            <p class="schedule-next" aria-live="polite">{scheduleSaved ? 'Saved. ' : ''}{schedule?.nextAlarm ? `Next wakeup: ${new Date(schedule.nextAlarm).toLocaleString([], { weekday: 'short', hour: 'numeric', minute: '2-digit' })} · only restarts if running` : 'No restart alarm scheduled.'}</p>
            {#if scheduleError}<p class="inline-error" role="alert">{scheduleError}</p>{/if}
            <small>Android force-stop disables alarms until you open the app again. Manufacturer battery restrictions can affect delivery.</small>
        </form>
    </details>

	<details class="panel share">
		<summary>
			<span>
				<strong>Connect &amp; share</strong>
				<small>QR code and coordinator link</small>
			</span>
		</summary>
		<div class="panel-content">
			{#if shareUrl}
				{#if qrSvg}
					<div class="qr" aria-label="QR code with the coordinator link">{@html qrSvg}</div>
				{/if}
				<button type="button" class="copy" onclick={() => copy(shareUrl)}>
					<code>{shareUrl}</code>
					<span>Copy</span>
				</button>
				<div class="share-actions">
					{#if 'share' in navigator}
						<button type="button" onclick={shareLink}>Share</button>
					{/if}
					<button type="button" onclick={openExternally}>Open</button>
				</div>
			{:else}
				<p class="loading">Start the coordinator once to create its identity.</p>
			{/if}
		</div>
	</details>

	<details class="panel diagnostics" ontoggle={toggleDiagnostics}>
		<summary>
			<span>
				<strong>Activity & logs</strong>
				<small>{status.state === 'running' ? 'Live diagnostics' : 'Coordinator diagnostics'}</small>
			</span>
		</summary>
		<div class="panel-content">
			{#if diagnostics}
				<div class="stats">
					<div><span>Uptime</span><strong>{formatUptime(diagnostics.startedAt)}</strong></div>
					<div><span>Storage</span><strong>{formatBytes(diagnostics.storageBytes)}</strong></div>
					<div><span>Key packages</span><strong>{count(diagnostics.keyPackages)}</strong></div>
					<div><span>Welcomes</span><strong>{count(diagnostics.welcomes)}</strong></div>
					<div><span>Join requests</span><strong>{count(diagnostics.joinRequests)}</strong></div>
					<div><span>Messages</span><strong>{count(diagnostics.groupMessages)}</strong></div>
				</div>
				<div class="log-heading">
					<span>Service log</span>
					<span class:live={status.state === 'running'} class="live-label">
						{status.state === 'running' ? 'Live' : 'Idle'}
					</span>
				</div>
				<pre bind:this={logView}>{diagnostics.logs.trimEnd() || 'No log entries yet.'}</pre>
			{:else if diagnosticsError}
				<p class="inline-error">{diagnosticsError}</p>
			{:else}
				<p class="loading">Loading diagnostics…</p>
			{/if}
		</div>
	</details>

	<form id="configuration" onsubmit={startCoordinator}>
		<details class="panel configuration">
			<summary>
				<span>
					<strong>Configuration</strong>
					<small>{relayCount} {relayCount === 1 ? 'relay' : 'relays'} · {maxAgeDays}-day retention</small>
				</span>
			</summary>
			<fieldset disabled={active || busy} aria-label="Coordinator configuration">
				<label>
					<span>Relay URLs</span>
					<textarea bind:value={relays} rows="3" required spellcheck="false"></textarea>
					<small>One wss:// URL per line</small>
				</label>

				<label>
					<span>Server name</span>
					<input bind:value={serverName} required maxlength="100" />
				</label>

				<label>
					<span>About <em>optional</em></span>
					<input bind:value={serverAbout} maxlength="280" />
				</label>

				<label>
					<span>Website <em>optional</em></span>
					<input bind:value={serverWebsite} inputmode="url" placeholder="https://" />
				</label>

				<div class="row">
					<label>
						<span>Retention days</span>
						<input bind:value={maxAgeDays} type="number" min="0" max="3650" required />
					</label>
					<label class="toggle">
						<input bind:checked={isAnnounced} type="checkbox" />
						<span>Announce server</span>
					</label>
				</div>

				<details class="identity-import">
					<summary>Import an existing identity</summary>
					<label>
						<span>Private key</span>
						<input
							bind:value={privateKey}
							type="password"
							inputmode="text"
							minlength="64"
							maxlength="64"
							autocomplete="off"
							placeholder="Leave empty to generate securely"
						/>
					</label>
				</details>

				<div class="danger-zone">
					<strong>Danger zone</strong>
					<div class="danger-row">
						<div>
							<strong>Erase coordinator data</strong>
							<small
								>Deletes all stored key packages, welcomes and messages. Keeps the identity and
									configuration.</small
								>
						</div>
						<button
							type="button"
							class="danger"
							onclick={() =>
								dangerConfirm === 'flush' ? void runDestructive(flushData) : armDanger('flush')}
						>
							{dangerConfirm === 'flush' ? 'Tap again to erase' : 'Erase'}
						</button>
					</div>
					<div class="danger-row">
						<div>
							<strong>Reset identity</strong>
							<small
								>Deletes the coordinator identity and all data. A new key is generated on next start —
									clients must use the new one.</small
								>
						</div>
						<button
							type="button"
							class="danger"
							onclick={() =>
								dangerConfirm === 'reset' ? void runDestructive(resetIdentity) : armDanger('reset')}
						>
							{dangerConfirm === 'reset' ? 'Tap again to reset' : 'Reset'}
						</button>
					</div>
				</div>
			</fieldset>
		</details>
	</form>
</main>

<style>
    .schedule-content { display: grid; gap: 12px; }
    .schedule-description, .schedule-next { color: #83a690; font-size: .75rem; line-height: 1.5; }
    .schedule-fields { padding: 0; gap: 12px; }
    .schedule-fields:disabled { opacity: .45; }
    .frequency { display: flex; gap: 6px; padding: 4px; border: 1px solid #2a3930; border-radius: 11px; background: #0a0e0c; }
    .frequency button { flex: 1; padding: 9px; background: transparent; color: #83a690; font-size: .78rem; font-weight: 700; }
    .frequency button.selected { background: #203e2c; color: #a3f0bd; }
    .schedule-content .action, .alarm-permission { margin-top: 0; }
    .schedule-content > small { font-size: .68rem; line-height: 1.5; }

	:global(*) { box-sizing: border-box; }
	:global(html) { color-scheme: dark; background: #090b0a; font-family: Inter, ui-sans-serif, system-ui, sans-serif; }
	:global(body) { margin: 0; min-width: 320px; background: radial-gradient(circle at 50% -20%, #193127 0, #090b0a 42%); color: #edf6f0; }
	:global(button), :global(input), :global(textarea) { font: inherit; }
	main { width: min(100%, 520px); min-height: 100vh; margin: auto; padding: max(28px, env(safe-area-inset-top)) 20px max(32px, env(safe-area-inset-bottom)); }
	header { display: flex; align-items: center; gap: 12px; margin-bottom: 24px; }
	h1, p { margin: 0; }
	h1 { font-size: 1.45rem; letter-spacing: -.03em; }
	.eyebrow, .label { color: #83a690; font-size: .72rem; font-weight: 700; letter-spacing: .12em; text-transform: uppercase; }
	.mark { position: relative; display: grid; width: 42px; height: 42px; place-items: center; border: 1px solid #355744; border-radius: 50%; }
	.mark::before, .mark span { content: ''; position: absolute; border: 2px solid #70e49b; border-radius: 50%; }
	.mark::before { width: 22px; height: 22px; }
	.mark span { width: 7px; height: 7px; background: #70e49b; }
	.status-dot { width: 9px; height: 9px; margin-left: auto; border-radius: 50%; background: #66706a; box-shadow: 0 0 0 5px #66706a18; }
	.status-dot.running { background: #70e49b; box-shadow: 0 0 0 5px #70e49b18, 0 0 16px #70e49b80; }
	.status-card, .panel { margin-bottom: 12px; border: 1px solid #202b24; border-radius: 16px; background: #101512e6; }
	.status-card { padding: 20px; }
	.status-heading strong { display: block; margin-top: 4px; font-size: 1.35rem; text-transform: capitalize; }
	.status-copy { margin-top: 3px; color: #718077; font-size: .78rem; }
	button { border: 0; border-radius: 10px; cursor: pointer; }
	button:disabled { cursor: not-allowed; opacity: .5; }
	.action { width: 100%; margin-top: 18px; padding: 13px; font-weight: 800; }
	.start { background: #70e49b; color: #07140c; }
	.stop { background: #3b2021; color: #ffb4b6; }
	.identity { margin-top: 18px; padding-top: 16px; border-top: 1px solid #202b24; }
	.identity-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
	.format-switch { display: flex; overflow: hidden; border: 1px solid #2a3930; border-radius: 8px; }
	.format-switch button { padding: 5px 9px; background: #0a0e0c; color: #718077; font-size: .68rem; font-weight: 600; }
	.format-switch button + button { border-left: 1px solid #2a3930; }
	.format-switch button.active { background: #70e49b; color: #07140c; }
	.copy { display: flex; width: 100%; align-items: center; gap: 12px; margin: 8px 0; padding: 10px; background: #0a0e0c; color: #d9e5dd; text-align: left; }
	.copy code { min-width: 0; overflow: hidden; font-size: .75rem; text-overflow: ellipsis; }
	.copy span { margin-left: auto; color: #70e49b; font-size: .75rem; }
	.qr { padding: 10px; margin-bottom: 12px; border-radius: 12px; background: #fff; }
	.qr :global(svg) { display: block; width: 100%; height: auto; }
	.share-actions { display: flex; gap: 8px; }
	.share-actions button { flex: 1; padding: 10px; background: #0a0e0c; border: 1px solid #2a3930; color: #d9e5dd; font-size: .8rem; font-weight: 600; }
	.reliability { display: flex; gap: 12px; align-items: center; margin-top: 18px; padding: 12px; border: 1px solid #514825; border-radius: 11px; background: #1c190d; }
	.reliability strong { color: #f0da76; font-size: .78rem; }
	.reliability p { margin-top: 2px; color: #a99f73; font-size: .7rem; line-height: 1.35; }
	.reliability button { flex: 0 0 auto; padding: 7px 10px; background: #f0da76; color: #1c190d; font-size: .7rem; font-weight: 700; }
	.error { margin-bottom: 12px; padding: 12px 14px; border: 1px solid #6b3034; border-radius: 10px; background: #261315; color: #ffb4b6; font-size: .82rem; overflow-wrap: anywhere; }
	form { margin: 0; }
	.panel { overflow: hidden; }
	.panel > summary { display: flex; align-items: center; padding: 16px 18px; cursor: pointer; list-style: none; }
	.panel > summary::-webkit-details-marker { display: none; }
	.panel > summary::after { content: '+'; margin-left: auto; color: #83a690; font-size: 1.35rem; font-weight: 300; transition: transform .15s ease; }
	.panel[open] > summary::after { transform: rotate(45deg); }
	.panel[open] > summary { border-bottom: 1px solid #202b24; }
	.panel > summary span { display: grid; gap: 3px; }
	.panel > summary strong { font-size: .9rem; }
	.panel > summary small { color: #68796e; font-size: .7rem; }
	.panel-content { padding: 16px; }
	.stats { display: grid; grid-template-columns: repeat(2, 1fr); gap: 8px; }
	.stats div { display: grid; gap: 4px; padding: 11px; border-radius: 9px; background: #0a0e0c; }
	.stats span, .log-heading { color: #718077; font-size: .68rem; }
	.stats strong { font-size: .86rem; }
	.log-heading { display: flex; align-items: center; margin: 18px 2px 8px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }
	.live-label { margin-left: auto; color: #718077; font-size: .62rem; letter-spacing: normal; }
	.live-label.live { color: #70e49b; }
	.live-label.live::before { content: ''; display: inline-block; width: 5px; height: 5px; margin-right: 5px; border-radius: 50%; background: currentColor; }
	pre { max-height: 300px; margin: 0; overflow: auto; padding: 12px; border: 1px solid #202b24; border-radius: 9px; background: #070a08; color: #aebbb3; font: .65rem/1.5 ui-monospace, SFMono-Regular, Consolas, monospace; white-space: pre-wrap; overflow-wrap: anywhere; }
	.loading, .inline-error { color: #718077; font-size: .78rem; }
	.inline-error { color: #ffb4b6; }
	fieldset { display: grid; gap: 15px; margin: 0; padding: 16px; border: 0; }
	label { display: grid; gap: 7px; color: #b9c9bf; font-size: .82rem; }
	label em { color: #667a6d; font-style: normal; }
	input, textarea { width: 100%; border: 1px solid #2a3930; border-radius: 9px; outline: 0; background: #0a0e0c; color: #edf6f0; padding: 11px 12px; }
	input:focus, textarea:focus { border-color: #70e49b; box-shadow: 0 0 0 3px #70e49b18; }
	textarea { resize: vertical; }
	small { color: #667a6d; }
	.row { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; align-items: end; }
	.toggle { display: flex; min-height: 42px; align-items: center; gap: 9px; }
	.toggle input { width: 18px; height: 18px; accent-color: #70e49b; }
	.identity-import { border-top: 1px solid #202b24; padding-top: 14px; }
	.identity-import summary { cursor: pointer; color: #83a690; font-size: .82rem; }
	.identity-import label { margin-top: 13px; }
	.danger-zone { display: grid; gap: 12px; margin-top: 16px; border-top: 1px solid #202b24; padding-top: 14px; }
	.danger-zone > strong { color: #ffb4b6; font-size: .72rem; font-weight: 700; letter-spacing: .12em; text-transform: uppercase; }
	.danger-row { display: flex; gap: 12px; align-items: center; justify-content: space-between; padding: 11px; border: 1px solid #4d2729; border-radius: 10px; background: #1a1011; }
	.danger-row strong { font-size: .8rem; }
	.danger-row small { display: block; margin-top: 2px; color: #a4757a; font-size: .68rem; line-height: 1.35; }
	.danger-row .danger { flex: 0 0 auto; padding: 8px 12px; background: #5c2125; color: #ffb4b6; font-size: .72rem; font-weight: 700; }
	@media (max-width: 390px) { .row { grid-template-columns: 1fr; } .reliability { align-items: flex-start; flex-direction: column; } }
</style>
