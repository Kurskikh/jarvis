import { writable, get } from "svelte/store"
import { invoke } from "@tauri-apps/api/core"
import { getCurrentWindow } from "@tauri-apps/api/window"

// ### IPC STORES ###

export type JarvisState = "disconnected" | "idle" | "listening" | "processing"

// answer to the reload_commands action, as reported by the running assistant.
//
// `ok` means the new list is LIVE, not that everything went well: `skipped`
// names packs that were dropped because their TOML does not parse, and
// retrainError means the commands are live but the intent classifier could not
// be rebuilt on them. Both travel with ok:true and both need saying out loud.
export type ReloadResult = {
    requestId: string | null
    ok: boolean
    packs: number
    commands: number
    retrained: boolean
    skipped: string[]
    retrainError: string | null
    error: string | null
}

// one LLM turn, as reported by the running assistant. `thinking` is true
// between llm_thinking and llm_answer; the two errors are the machine-readable
// code (for the localized headline) and the composed English detail from Rust.
export type LlmTurn = {
    requestId: string
    prompt: string
    answer: string | null
    model: string
    elapsedMs: number
    errorCode: string | null
    error: string | null
    thinking: boolean
}

export const jarvisState = writable<JarvisState>("disconnected")
export const ipcConnected = writable(false)
export const lastRecognizedText = writable("")
export const lastExecutedCommand = writable("")
export const lastError = writable("")
export const lastReload = writable<ReloadResult | null>(null)
export const llmTurn = writable<LlmTurn | null>(null)

// ### CONNECTION ###

const IPC_URL = "ws://127.0.0.1:9712"
const RECONNECT_DELAY = 5000

let ws: WebSocket | null = null
let reconnectTimer: ReturnType<typeof setTimeout> | null = null
let manualDisconnect = false
let enabled = false  // only connect when enabled

export function enableIpc() {
    enabled = true
    // disableIpc() latched this and nothing ever reset it, so every page that
    // re-enabled IPC after a stop got a socket that could never reconnect
    manualDisconnect = false
    connectIpc()
}

export function disableIpc() {
    enabled = false
    disconnectIpc()
}

export function connectIpc(port: number = 9712) {
    if (ws?.readyState === WebSocket.OPEN) return

    ws = new WebSocket(`ws://127.0.0.1:${port}`)

    ws.onopen = () => {
        ipcConnected.set(true)
        jarvisState.set("idle")
        console.log("[IPC] connected")
    }

    ws.onclose = () => {
        ipcConnected.set(false)
        // the assistant is the only thing that can end a turn. once the socket
        // is gone no llm_answer can arrive, so a {thinking:true} left in the
        // store is a spinner that runs until the next utterance - or forever,
        // if the process that would have sent one is the one that just died.
        llmTurn.set(null)
        console.log("[IPC] disconnected")
    }

    ws.onerror = (err) => {
        console.error("[IPC] error:", err)
    }

    ws.onmessage = (event) => {
        try {
            const msg = JSON.parse(event.data)
            handleEvent(msg)
        } catch (e) {
            console.error("[IPC] failed to parse message:", e)
        }
    }
}

function scheduleReconnect() {
    if (reconnectTimer || manualDisconnect || !enabled) return

    console.log(`IPC: Will retry in ${RECONNECT_DELAY / 1000}s...`)
    reconnectTimer = setTimeout(() => {
        reconnectTimer = null
        connectIpc()
    }, RECONNECT_DELAY)
}

export function disconnectIpc() {
    manualDisconnect = true

    if (reconnectTimer) {
        clearTimeout(reconnectTimer)
        reconnectTimer = null
    }

    if (ws) {
        ws.close()
        ws = null
    }

    ipcConnected.set(false)
    jarvisState.set("disconnected")
    // ws.close() above is asynchronous and onclose is not guaranteed to run
    // after the handler was detached, so the same clear is repeated here.
    // disableIpc() fires from the home route the moment the assistant stops,
    // i.e. on the ordinary Stop button, not only on a crash.
    llmTurn.set(null)
}

// ### EVENT HANDLING ###

function handleEvent(data: any) {
    console.log("IPC: Event", data.event, data)

    switch (data.event) {
        case "wake_word_detected":
        case "listening":
            jarvisState.set("listening")
            break

        case "speech_recognized":
            lastRecognizedText.set(data.text || "")
            jarvisState.set("processing")
            // a new utterance retires the previous answer. nothing else clears
            // this store, and a stale answer would sit on screen forever.
            llmTurn.set(null)
            break

        case "command_executed":
            lastExecutedCommand.set(data.id || "")
            break

        case "idle":
            jarvisState.set("idle")
            break

        case "error":
            lastError.set(data.message || "Unknown error")
            break

        case "started":
            jarvisState.set("idle")
            break

        case "stopping":
            jarvisState.set("disconnected")
            // announced shutdown: whatever was in flight will never answer
            llmTurn.set(null)
            break

        case "pong":
            // connection verified
            break

        case "reveal_window":
            // bring window to foreground
            revealWindow()
            break

        case "commands_reloaded":
            lastReload.set({
                requestId: data.request_id ?? null,
                ok: !!data.ok,
                packs: data.packs ?? 0,
                commands: data.commands ?? 0,
                retrained: !!data.retrained,
                skipped: Array.isArray(data.skipped) ? data.skipped : [],
                retrainError: data.retrain_error ?? null,
                error: data.error ?? null
            })
            break

        case "llm_thinking":
            llmTurn.set({
                requestId: data.request_id ?? "",
                prompt: data.prompt ?? "",
                answer: null,
                model: "",
                elapsedMs: 0,
                errorCode: null,
                error: null,
                thinking: true
            })
            break

        case "llm_answer": {
            // a late answer from a superseded turn must not overwrite a newer
            // one. jarvis-app already drops stale answers by generation, but
            // the socket can reconnect mid-turn and ordering across a reconnect
            // is not guaranteed. an empty request_id is the pre-flight
            // NotConfigured case, which has no matching llm_thinking - let it
            // through.
            const current = get(llmTurn)
            const rid = data.request_id ?? ""
            if (current && rid && current.requestId && current.requestId !== rid) break
            llmTurn.set({
                requestId: rid,
                prompt: data.prompt ?? "",
                answer: data.answer ?? null,
                model: data.model ?? "",
                elapsedMs: data.elapsed_ms ?? 0,
                errorCode: data.error_code ?? null,
                error: data.error ?? null,
                thinking: false
            })
            break
        }
    }
}

// resolves true as soon as the socket is up, false on timeout. the command
// editor uses it to give a just-launched assistant a moment before it decides
// the save could not be applied live.
export function waitForIpcConnected(timeoutMs = 1500): Promise<boolean> {
    if (get(ipcConnected)) return Promise.resolve(true)

    return new Promise(resolve => {
        let settled = false
        let unsub: (() => void) | null = null
        let timer: ReturnType<typeof setTimeout> | null = null

        const finish = (ok: boolean) => {
            if (settled) return
            settled = true

            if (timer !== null) clearTimeout(timer)
            queueMicrotask(() => unsub?.())

            resolve(ok)
        }

        timer = setTimeout(() => finish(false), timeoutMs)
        unsub = ipcConnected.subscribe(value => { if (value) finish(true) })
    })
}

// resolves on the commands_reloaded event carrying `requestId`, or null on
// timeout.
//
// sendAction() is fire-and-forget and returns false silently when the socket is
// closed, so this is the only way the command editor can tell "written to disk"
// apart from "actually live in the running assistant".
//
// the id match is not decoration: without it a second save, a second window or
// a `reload` typed into jarvis-cli resolves this wait, and save #2's banner
// ends up reporting save #1's outcome.
export function awaitReload(requestId: string, timeoutMs = 15000): Promise<ReloadResult | null> {
    return new Promise(resolve => {
        let settled = false
        let unsub: (() => void) | null = null
        let timer: ReturnType<typeof setTimeout> | null = null

        const finish = (result: ReloadResult | null) => {
            if (settled) return
            settled = true

            if (timer !== null) clearTimeout(timer)
            // subscribe() fires synchronously, before unsub has been assigned
            queueMicrotask(() => unsub?.())

            resolve(result)
        }

        timer = setTimeout(() => finish(null), timeoutMs)

        // the value already in the store belongs to an earlier reload
        let replay = true
        unsub = lastReload.subscribe(value => {
            if (replay) {
                replay = false
                return
            }
            if (value && value.requestId === requestId) finish(value)
        })
    })
}

// ### ACTIONS ###

export function sendAction(action: string, payload: Record<string, any> = {}) {
    if (ws?.readyState !== WebSocket.OPEN) {
        return false
    }

    ws.send(JSON.stringify({ action, ...payload }))
    return true
}

export function stopJarvisApp() {
    return sendAction("stop")
}

// returns the request id the answering commands_reloaded event will echo, or
// null when the socket is not open. pass it straight to awaitReload().
export function reloadCommands(): string | null {
    const requestId = `reload-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`

    return sendAction("reload_commands", { request_id: requestId }) ? requestId : null
}

export function sendIpcMessage(message: object): Promise<void> {
    return new Promise((resolve, reject) => {
        if (!ws || ws.readyState !== WebSocket.OPEN) {
            reject(new Error("IPC not connected"))
            return
        }

        try {
            ws.send(JSON.stringify(message))
            resolve()
        } catch (err) {
            reject(err)
        }
    })
}

export function sendTextCommand(text: string): boolean {
    return sendAction("text_command", { text })
}

// tell the running assistant to re-read the LLM settings from app.db.
//
// jarvis-app loads app.db once at startup and this window is a different
// process, so without this an llm_* value saved here never reaches it. returns
// false when the socket is not open - the caller must then say a restart is
// needed instead of pretending the save is live.
export function reloadSettings(): boolean {
    return sendAction("reload_settings")
}

async function revealWindow() {
    try {
        const window = getCurrentWindow()
        await window.show()
        await window.unminimize()
        await window.setFocus()
    } catch (e) {
        console.error("[IPC] Failed to reveal window:", e)
    }
}
