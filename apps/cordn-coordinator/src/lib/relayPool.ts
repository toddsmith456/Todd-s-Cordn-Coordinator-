export type RelayPoolEntry = {
	url: string;
	name: string;
	note: string;
	ephemeralAdvertised: boolean;
};

// Vetted public relays for coordinator defaults/browsing.
// Criteria: no NIP-42 auth required, no payment/restricted writes indicated by NIP-11,
// good reputation/uptime, and suitable for ContextVM coordinator traffic (including
// ephemeral kinds such as 25910). NIP-16 advertisement is noted where observed; relays
// without explicit NIP-16 advertisement should be live-tested before being relied on.
export const DEFAULT_RELAY_POOL: RelayPoolEntry[] = [
	{ url: 'wss://relay.primal.net', name: 'Primal', note: 'Popular public strfry relay.', ephemeralAdvertised: false },
	{ url: 'wss://nostr.mom', name: 'nostr.mom', note: 'Public strfry relay.', ephemeralAdvertised: false },
	{ url: 'wss://offchain.pub', name: 'Offchain', note: 'Public strfry relay.', ephemeralAdvertised: false },
	{ url: 'wss://nostr.bitcoiner.social', name: 'Bitcoiner.social', note: 'Public strfry relay.', ephemeralAdvertised: false },
	{ url: 'wss://nostr.data.haus', name: 'data.haus', note: 'Public strfry relay.', ephemeralAdvertised: false },
	{ url: 'wss://nostr.oxtr.dev', name: '0xtr', note: 'Public strfry relay.', ephemeralAdvertised: false },
	{ url: 'wss://relay.nostr.wirednet.jp', name: 'wirednet.jp', note: 'Public strfry relay.', ephemeralAdvertised: false },
	{ url: 'wss://relay.nostrplebs.com', name: 'NostrPlebs', note: 'Public strfry relay.', ephemeralAdvertised: false },
	{ url: 'wss://relay.nos.social', name: 'nos.social', note: 'Public strfry relay.', ephemeralAdvertised: true },
	{ url: 'wss://nostr.lopp.social', name: 'lopp.social', note: 'Public strfry relay.', ephemeralAdvertised: true },
	{ url: 'wss://nostr.sathoarder.com', name: 'Sathoarder', note: 'Public strfry relay.', ephemeralAdvertised: true },
	{ url: 'wss://nostr-01.yakihonne.com', name: 'YakiHonne 01', note: 'Public strfry relay.', ephemeralAdvertised: true },
	{ url: 'wss://nostr-02.yakihonne.com', name: 'YakiHonne 02', note: 'Public strfry relay.', ephemeralAdvertised: true },
	{ url: 'wss://nostr.0x7e.xyz', name: '0x7e', note: 'Open nostr-rs-relay.', ephemeralAdvertised: true },
	{ url: 'wss://nostr.kosmos.org', name: 'Kosmos', note: 'Public strfry relay.', ephemeralAdvertised: false },
	{ url: 'wss://soloco.nl', name: 'Soloco', note: 'Public strfry relay.', ephemeralAdvertised: false },
	{ url: 'wss://relay.letsfo.com', name: 'Let\'s Fo', note: 'Public strfry relay.', ephemeralAdvertised: false },
	{ url: 'wss://relay.snort.social', name: 'Snort', note: 'Public Snort relay; live-test ephemeral support.', ephemeralAdvertised: false },
	{ url: 'wss://cordn.feeds.relay.tools', name: 'Cordn Feeds', note: 'Cordn relay.tools relay.', ephemeralAdvertised: false },
	{ url: 'wss://relay.contextvm.org', name: 'ContextVM', note: 'ContextVM relay.', ephemeralAdvertised: false },
	{ url: 'wss://relay2.contextvm.org', name: 'ContextVM 2', note: 'ContextVM relay.', ephemeralAdvertised: false }
];

export function pickRandomRelays(pool: RelayPoolEntry[], count: number): string[] {
	const copy = [...pool];
	for (let i = copy.length - 1; i > 0; i--) {
		const j = Math.floor(Math.random() * (i + 1));
		[copy[i], copy[j]] = [copy[j], copy[i]];
	}
	return copy.slice(0, count).map((r) => r.url);
}

export function normalizeRelayUrl(url: string): string {
	return url.trim();
}

export function parseRelayInput(value: string): string[] {
	const seen = new Set<string>();
	const out: string[] = [];
	for (const raw of value.split(/[\n,]/)) {
		const relay = normalizeRelayUrl(raw);
		if (!relay || seen.has(relay)) continue;
		seen.add(relay);
		out.push(relay);
	}
	return out;
}
