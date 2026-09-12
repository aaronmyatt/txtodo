// QR-scanning camera loop: getUserMedia + jsQR, isolated from Devices.svelte so the component only
// has to start/stop it and react to a decoded string.
// getUserMedia (browser API, not Node — fine under the "no fs/no raw socket" bundle rule):
// https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia
// jsQR: https://github.com/cozmo/jsQR
import jsQR from "jsqr";

export interface QrScanner {
	/** Stops the camera and the decode loop. Safe to call more than once. */
	stop(): void;
}

/**
 * Opens the camera into `video` and decodes frames drawn onto `canvas` at animation-frame rate.
 * Calls `onDecode` at most once per scanner (the loop stops itself right after a hit) — the caller
 * starts a fresh scanner if it wants to keep looking (e.g. after rejecting an invalid payload).
 * Calls `onError` and never starts the loop if the camera can't be opened.
 */
export async function startQrScanner(
	video: HTMLVideoElement,
	canvas: HTMLCanvasElement,
	onDecode: (text: string) => void,
	onError: (message: string) => void
): Promise<QrScanner> {
	let stopped = false;
	let rafId = 0;
	let stream: MediaStream;

	function stop() {
		if (stopped) return;
		stopped = true;
		cancelAnimationFrame(rafId);
		stream?.getTracks().forEach((track) => track.stop());
		video.srcObject = null;
	}

	try {
		stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: "environment" } });
	} catch (e) {
		onError(e instanceof Error ? e.message : String(e));
		return { stop() {} };
	}

	video.srcObject = stream;
	await video.play();
	const ctx = canvas.getContext("2d", { willReadFrequently: true });

	function tick() {
		if (stopped) return;
		if (ctx && video.readyState === video.HAVE_ENOUGH_DATA) {
			canvas.width = video.videoWidth;
			canvas.height = video.videoHeight;
			ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
			const frame = ctx.getImageData(0, 0, canvas.width, canvas.height);
			const code = jsQR(frame.data, frame.width, frame.height);
			if (code?.data) {
				stop();
				onDecode(code.data);
				return;
			}
		}
		rafId = requestAnimationFrame(tick);
	}
	rafId = requestAnimationFrame(tick);

	return { stop };
}
