<script lang="ts">
	// Pairing pane (plan M7, design §4/§7): show this device's QR (pair_offer) or scan a peer's
	// (getUserMedia + jsQR → pair_accept), then require an explicit tap on *this* device before
	// pair_confirm_sas() — never an automatic confirm. All crypto and pairing state live in the
	// daemon (crates/txtodo-sync, crates/txtodo-daemon); this component only calls the three
	// Tauri commands and renders their results.
	import { onDestroy, tick } from "svelte";
	import QRCode from "qrcode"; // https://github.com/soldair/node-qrcode
	import { pairAccept, pairConfirmSas, pairOffer } from "./api";
	import { startQrScanner, type QrScanner } from "./camera";
	import { decodePairOfferQr, encodePairOfferQr } from "./qr";
	import {
		MAX_CONCURRENT_PAIRINGS,
		PAIRING_WINDOW_MS,
		classifyPairingError,
		pairingWindowRemainingMs
	} from "./pairing";
	import type { PairOffer } from "./types";

	type Mode = "idle" | "offer" | "scan";
	type OfferState = "loading" | "active" | "expired" | "too-many" | "error";
	type ScanState = "requesting-camera" | "scanning" | "invalid-code" | "accepting" | "accept-error" | "camera-error";
	type ConfirmState = "none" | "sas-ready" | "armed" | "confirming" | "confirmed" | "error";

	let mode = $state<Mode>("idle");

	// --- Offer (show my QR) ---
	let offerState = $state<OfferState>("loading");
	let offer = $state<PairOffer | null>(null);
	let offerQrDataUrl = $state("");
	let offerError = $state("");
	let openedAtMs = $state(0);
	let remainingMs = $state(PAIRING_WINDOW_MS);
	let countdownTimer: ReturnType<typeof setInterval> | undefined;

	// --- Scan (read a peer's QR) ---
	let scanState = $state<ScanState>("requesting-camera");
	let scanError = $state("");
	let videoEl = $state<HTMLVideoElement>();
	let canvasEl = $state<HTMLCanvasElement>();
	let scanner: QrScanner | null = null;
	let scannedCode = $state("");

	// --- SAS confirmation (shared by both roles: one instance of this component is one device) ---
	let confirmState = $state<ConfirmState>("none");
	let sas = $state("");
	let confirmError = $state("");

	function resetPairingUiState() {
		stopCountdown();
		scanner?.stop();
		scanner = null;
		confirmState = "none";
		sas = "";
		confirmError = "";
		scanError = "";
		scannedCode = "";
	}

	function goIdle() {
		resetPairingUiState();
		mode = "idle";
	}

	// ---- Offer flow ----

	async function startOffer() {
		resetPairingUiState();
		mode = "offer";
		offerState = "loading";
		offerError = "";
		try {
			const o = await pairOffer();
			offer = o;
			offerQrDataUrl = await QRCode.toDataURL(encodePairOfferQr(o));
			openedAtMs = Date.now();
			remainingMs = PAIRING_WINDOW_MS;
			offerState = "active";
			startCountdown();
		} catch (e) {
			const message = e instanceof Error ? e.message : String(e);
			offerError = message;
			const kind = classifyPairingError(message);
			offerState = kind === "too-many-pairings" ? "too-many" : kind === "window-expired" ? "expired" : "error";
		}
	}

	function startCountdown() {
		stopCountdown();
		countdownTimer = setInterval(() => {
			remainingMs = pairingWindowRemainingMs(openedAtMs, Date.now());
			if (remainingMs <= 0) {
				offerState = "expired";
				stopCountdown();
			}
		}, 1000);
	}

	function stopCountdown() {
		if (countdownTimer) clearInterval(countdownTimer);
		countdownTimer = undefined;
	}

	// ---- Scan flow ----

	async function startScan() {
		resetPairingUiState();
		mode = "scan";
		scanState = "requesting-camera";
		await tick(); // let the <video>/<canvas> mount before we bind to them
		if (!videoEl || !canvasEl) return;
		scanner = await startQrScanner(videoEl, canvasEl, handleDecoded, (message) => {
			scanError = message;
			scanState = "camera-error";
		});
		if (scanState === "requesting-camera") scanState = "scanning";
	}

	async function resumeScan() {
		if (!videoEl || !canvasEl) return;
		scanner = await startQrScanner(videoEl, canvasEl, handleDecoded, (message) => {
			scanError = message;
			scanState = "camera-error";
		});
		scanState = "scanning";
	}

	/** The security invariant this file must uphold: only a payload shaped exactly like
	 * `PairOffer` (see `qr.ts`) ever reaches `pair_accept`; anything else is rejected and scanning
	 * resumes, no partial trust extended. */
	async function handleDecoded(text: string) {
		const parsed = decodePairOfferQr(text);
		if (!parsed) {
			scanState = "invalid-code";
			await resumeScan();
			return;
		}
		scannedCode = text;
		scanState = "accepting";
		try {
			const result = await pairAccept(text);
			sas = result.sas;
			confirmState = "sas-ready";
		} catch (e) {
			scanError = e instanceof Error ? e.message : String(e);
			scanState = "accept-error";
		}
	}

	// ---- Shared SAS confirmation (no auto-confirm, ever) ----

	/** First tap: arms the confirm action but calls nothing yet. */
	function armConfirm() {
		confirmState = "armed";
	}

	function cancelConfirm() {
		confirmState = sas ? "sas-ready" : "none";
	}

	/** Second, distinct tap: the only place `pair_confirm_sas()` is called. */
	async function confirmMatch() {
		confirmState = "confirming";
		confirmError = "";
		try {
			const result = await pairConfirmSas();
			if (sas && result.sas !== sas) {
				confirmError = "This device's SAS changed on confirm — do not trust this pairing.";
				confirmState = "error";
				return;
			}
			sas = result.sas;
			confirmState = "confirmed";
		} catch (e) {
			confirmError = e instanceof Error ? e.message : String(e);
			confirmState = "error";
		}
	}

	onDestroy(resetPairingUiState);
</script>

<section class="devices">
	<h2>Pair a device</h2>

	{#if mode === "idle"}
		<div class="actions">
			<button type="button" onclick={startOffer}>Show my code</button>
			<button type="button" onclick={startScan}>Scan a peer's code</button>
		</div>
	{:else}
		<button type="button" class="back" onclick={goIdle}>&larr; Back</button>
	{/if}

	{#if mode === "offer"}
		<div class="pane" aria-live="polite">
			{#if offerState === "loading"}
				<p>Requesting a pairing code…</p>
			{:else if offerState === "active" && offer}
				<img src={offerQrDataUrl} alt="Pairing QR code" width="220" height="220" />
				<p class="countdown">Expires in {Math.ceil(remainingMs / 1000)}s</p>
				<p class="hint">Scan this on the other device. It never carries a group key.</p>
				{@render sasConfirm("Peer scanned it? Reveal this device's SAS and confirm.")}
			{:else if offerState === "expired"}
				<p class="state-expired">Pairing window closed.</p>
				<p class="hint">The code expires after {PAIRING_WINDOW_MS / 1000}s.</p>
				<button type="button" onclick={startOffer}>Generate a new code</button>
			{:else if offerState === "too-many"}
				<p class="state-blocked">
					Too many pairings — only {MAX_CONCURRENT_PAIRINGS} can be open at once.
				</p>
				<p class="hint">{offerError}</p>
				<button type="button" onclick={startOffer}>Try again</button>
			{:else if offerState === "error"}
				<p class="state-error">Could not start pairing: {offerError}</p>
				<button type="button" onclick={startOffer}>Retry</button>
			{/if}
		</div>
	{/if}

	{#if mode === "scan"}
		<div class="pane" aria-live="polite">
			<video bind:this={videoEl} class="camera" muted playsinline></video>
			<canvas bind:this={canvasEl} hidden></canvas>

			{#if scanState === "requesting-camera"}
				<p>Requesting camera access…</p>
			{:else if scanState === "scanning"}
				<p class="hint">Point the camera at the other device's code.</p>
			{:else if scanState === "invalid-code"}
				<p class="state-error">That code isn't a valid pairing offer. Still scanning…</p>
			{:else if scanState === "accepting"}
				<p>Code recognized — starting the handshake…</p>
			{:else if scanState === "accept-error"}
				<p class="state-error">Pairing was refused: {scanError}</p>
				<button type="button" onclick={startScan}>Scan again</button>
			{:else if scanState === "camera-error"}
				<p class="state-error">Could not open the camera: {scanError}</p>
				<button type="button" onclick={startScan}>Retry</button>
			{/if}

			{#if confirmState !== "none"}
				{@render sasConfirm("Compare the SAS out loud with your peer, then confirm.")}
			{/if}
		</div>
	{/if}
</section>

{#snippet sasConfirm(prompt: string)}
	<div class="sas-confirm">
		{#if confirmState === "sas-ready" || confirmState === "armed"}
			<p class="hint">{prompt}</p>
		{/if}
		{#if sas}
			<p class="sas">{sas}</p>
		{/if}
		{#if confirmState === "sas-ready"}
			<button type="button" onclick={armConfirm}>Confirm match</button>
		{:else if confirmState === "armed"}
			<p class="state-warning">This confirms the pairing on this device. Continue?</p>
			<button type="button" onclick={confirmMatch}>Yes, confirm match</button>
			<button type="button" onclick={cancelConfirm}>Cancel</button>
		{:else if confirmState === "confirming"}
			<p>Confirming…</p>
		{:else if confirmState === "confirmed"}
			<p class="state-ok">Confirmed on this device. Waiting on the other device to confirm too.</p>
		{:else if confirmState === "error"}
			<p class="state-error">{confirmError}</p>
			<button type="button" onclick={cancelConfirm}>Dismiss</button>
		{/if}
	</div>
{/snippet}

<style>
	.devices {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}
	.actions {
		display: flex;
		gap: 0.5rem;
	}
	.back {
		align-self: flex-start;
	}
	.pane {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 0.5rem;
		border: 1px solid #d1d5db;
		border-radius: 8px;
		padding: 1rem;
	}
	.camera {
		width: 100%;
		max-width: 320px;
		border-radius: 6px;
		background: #111827;
	}
	.hint {
		color: #6b7280;
		font-size: 0.9rem;
	}
	.sas {
		font-size: 1.1rem;
		font-weight: 600;
		letter-spacing: 0.02em;
	}
	.sas-confirm {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 0.4rem;
	}
	.state-expired,
	.state-blocked,
	.state-error {
		color: #b91c1c;
		font-weight: 600;
	}
	.state-warning {
		color: #92400e;
	}
	.state-ok {
		color: #15803d;
		font-weight: 600;
	}
	.countdown {
		font-variant-numeric: tabular-nums;
	}
</style>
