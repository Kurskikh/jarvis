<script lang="ts">
    import { onMount } from "svelte"
    import { invoke } from "@tauri-apps/api/core"
    import { goto, beforeUrlChange } from "@roxi/routify"
    import { setTimeout } from "worker-timers"

    import { showInExplorer } from "@/functions"
    import {
        translations,
        translate,
        ipcConnected,
        enableIpc,
        reloadCommands,
        awaitReload,
        waitForIpcConnected,
        refreshCommandsCount
    } from "@/stores"

    import HDivider from "@/components/elements/HDivider.svelte"

    import {
        Notification,
        Alert,
        Button,
        ActionIcon,
        Text,
        Badge,
        Paper,
        Group,
        Space,
        Input,
        InputWrapper,
        TextInput,
        Textarea,
        NumberInput,
        NativeSelect,
        Switch,
        Accordion,
        Modal,
        Loader,
        Divider
    } from "@svelteuidev/core"

    import {
        Plus,
        Trash,
        Reload,
        Check,
        CrossCircled,
        InfoCircled,
        ExclamationTriangle,
        MagnifyingGlass
    } from "radix-icons-svelte"

    // ### TYPES
    // mirror of jarvis-core commands/structs.rs. `type` is the serde rename of
    // cmd_type; the runtime caches on JCommand are #[serde(skip)] and never
    // appear on the wire in either direction.
    interface Slot {
        entity: string
        context: string[]
    }

    interface JCommand {
        id: string
        type: string
        description: string
        exe_path: string
        exe_args: string[]
        cli_cmd: string
        cli_args: string[]
        script: string
        sandbox: string
        timeout: number
        sounds: Record<string, string[]>
        phrases: Record<string, string[]>
        slots: Record<string, Slot>
    }

    interface CommandPack {
        name: string
        path: string
        commands: JCommand[]
        error: string | null
        // hash of command.toml as it was read; echoed back on save so a
        // concurrent hand edit in the same folder is refused, not overwritten
        revision: string
        // false when the folder name is outside what the editor can address.
        // such a pack runs fine in the assistant but is read-only here.
        managed: boolean
    }

    interface PackValidation {
        errors: string[]
        warnings: string[]
    }

    interface PackFiles {
        scripts: string[]
        executables: string[]
    }

    interface SlotRow {
        name: string
        entity: string
        contextText: string
    }

    // ### i18n
    // every key this page uses lives in en/ru/ua.ftl. translate() returns the
    // key itself when a bundle is missing one, which is loud enough to notice.
    $: t = (key: string) => translate($translations, key)

    // ### STATE
    // i18n::SUPPORTED_LANGUAGES. a pack may carry other keys; openCommand()
    // unions them in so a save can never silently drop a language
    const LANGS = ["ru", "en", "ua"]

    let view: "packs" | "pack" | "command" = "packs"

    let packs: CommandPack[] = []
    let packFilter = ""
    let listError = ""
    // the pack list starts empty and list_command_packs is a round trip, so
    // without this the page renders "No command packs found." at every load
    let loadingPacks = true

    // working copy of the open pack. nothing here touches disk until savePack()
    let pack: CommandPack | null = null
    let packOriginal = ""
    let cmdIndex = -1

    let packFiles: PackFiles = { scripts: [], executables: [] }
    let soundNames: Record<string, string[]> = {}

    let rawMode = false
    let rawText = ""
    let rawOriginal = ""

    // form buffers, derived once in openCommand() and projected back into the
    // draft by projectPack(). a Textarea with one entry per line is the only
    // usable affordance for a Vec<String> in a 490px window.
    let phraseText: Record<string, string> = {}
    let soundText: Record<string, string> = {}
    let exeArgsText = ""
    let cliArgsText = ""
    let slotRows: SlotRow[] = []
    let formLangs: string[] = LANGS

    // languages the open command carries with a DELIBERATELY empty array.
    // resolve_localized() treats `ru = []` as silence for Russian and an absent
    // `ru` as "use the English entry", and an empty textarea cannot tell the two
    // apart on its own - so the distinction is remembered here instead.
    let emptyPhraseLangs = new Set<string>()
    let emptySoundLangs = new Set<string>()

    let commandTypes: string[] = []
    let sandboxLevels: string[] = []
    let defaultTimeout = 10000

    let saving = false
    let reloading = false
    // the success banner's title, "" when hidden - a delete must not claim a save
    let savedMsg = ""
    // the red banner's body and its TITLE. the title used to be hardcoded to
    // "The pack was not saved", which four unrelated failures - a failed read, a
    // failed create, a failed delete, a refused reload - all reported under.
    let errorMsg = ""
    let errorTitle = ""
    let validationErrors: string[] = []
    let warnings: string[] = []
    let reloadNote = ""
    let reloadOk = false
    let reloadSkipped: string[] = []

    // one Modal instance, driven by flat fields. no discriminated union: the
    // Svelte template does not narrow one, and every read below would need a
    // cast that svelte-check would then have to trust
    let modalKind: "" | "createPack" | "deletePack" | "deleteCommand" | "discard" = ""
    let modalName = ""
    let modalPack = ""
    let modalTyped = ""
    let modalIndex = -1
    let modalTo: "packs" | "url" = "packs"
    let pendingUrl = ""

    // ### DERIVED
    $: cmd = (pack && cmdIndex >= 0 && cmdIndex < pack.commands.length)
        ? pack.commands[cmdIndex]
        : null

    // the pack as it WOULD be written right now, form buffers included.
    // pure - it never mutates `pack`, which is what makes it safe to use as a
    // reactive dependency. naming every buffer in the call is deliberate: that
    // is how Svelte learns to recompute `dirty` while the user types.
    $: projected = projectPack(pack, cmd, cmdIndex, phraseText, soundText,
                               exeArgsText, cliArgsText, slotRows, formLangs,
                               emptyPhraseLangs, emptySoundLangs)

    // tracked separately, and OR-ed for the navigation guard: toggling the raw
    // switch does not clear either buffer, so a draft in the mode that is not
    // currently on screen must still block a silent navigation away
    $: rawDirty = pack !== null && rawText !== rawOriginal
    $: structDirty = projected !== null && canon(projected) !== packOriginal
    $: dirty = rawDirty || structDirty

    $: rawLocked = pack !== null && pack.error !== null

    $: filteredPacks = packs.filter(p =>
        !packFilter || p.name.toLowerCase().includes(packFilter.toLowerCase()))

    // NativeSelect renders selected={item.value === value} and does not resync
    // when options are patched in later, so every data-driven picker below is
    // wrapped in {#key ...} - never keyed on the value.
    $: typeSelectData = commandTypes.map(v => ({ label: typeLabel(v), value: v }))
    $: sandboxSelectData = sandboxLevels.map(v => ({ label: t(`commands-sandbox-${v}`), value: v }))

    // the current value always stays in the list, so opening a command whose
    // script or executable is missing does not silently rewrite it to option[0]
    $: scriptData = [
        { label: "script.lua", value: "" },
        ...uniq([cmd ? cmd.script : "", ...packFiles.scripts]).map(v => ({ label: v, value: v }))
    ]
    $: exeData = [
        { label: "—", value: "" },
        ...uniq([cmd ? cmd.exe_path : "", ...packFiles.executables]).map(v => ({ label: v, value: v }))
    ]

    // the {#key} expressions. keyed on the option VALUES, not on the array:
    // scriptData and exeData both read `cmd`, and every bind:value={cmd.*}
    // invalidates `cmd`, so keying on the array remounted the select on every
    // keystroke anywhere in the form - and picking a script, which writes
    // cmd.script, tore down the very select the user had just used.
    $: typeKey = typeSelectData.map(d => d.value).join("\u0000")
    $: sandboxKey = sandboxSelectData.map(d => d.value).join("\u0000")
    $: scriptKey = scriptData.map(d => d.value).join("\u0000")
    $: exeKey = exeData.map(d => d.value).join("\u0000")

    $: createNameOk = /^[A-Za-z0-9_-]{1,64}$/.test(modalName.trim())
        && !modalName.trim().startsWith("-")
        && !packs.some(p => p.name.toLowerCase() === modalName.trim().toLowerCase())

    // ### HELPERS
    const toLines = (values?: string[]) => (values ?? []).join("\n")

    // for phrases and sounds, where a blank line is noise and surrounding
    // whitespace is never meaningful
    const fromLines = (text: string) => text.split("\n").map(x => x.trim()).filter(Boolean)

    // for exe_args / cli_args, where BOTH are meaningful: these go to a process
    // verbatim. trimming rewrote `" /im "` to `"/im"` and filtering dropped an
    // intentionally empty positional argument. the exact inverse of join("\n"),
    // except that "" is zero arguments rather than one empty one.
    const fromArgLines = (text: string) => text === "" ? [] : text.split("\n")

    const fromCsv = (text: string) => text.split(",").map(x => x.trim()).filter(Boolean)

    function uniq(values: string[]): string[] {
        return Array.from(new Set(values.filter(Boolean)))
    }

    // an unknown type (a hand-edited pack) must not render "cmdtype-foo"
    function typeLabel(value: string): string {
        const key = `cmdtype-${value}`
        const label = t(key)
        return label === key ? value : label
    }

    // stable stringify. the maps come from Rust HashMaps, whose key order is
    // arbitrary, so a plain JSON.stringify would report a freshly opened
    // command as dirty before the user touched anything.
    function canon(value: unknown): string {
        return JSON.stringify(value, (_key, v) => {
            if (v && typeof v === "object" && !Array.isArray(v)) {
                const sorted: Record<string, unknown> = {}
                for (const k of Object.keys(v as Record<string, unknown>).sort()) {
                    sorted[k] = (v as Record<string, unknown>)[k]
                }
                return sorted
            }
            return v
        })
    }

    // an empty textarea removes the language key UNLESS the command already
    // carried that key as an empty array - see emptyPhraseLangs above.
    function linesToMap(
        buffer: Record<string, string>,
        langs: string[],
        keepEmpty: Set<string>
    ): Record<string, string[]> {
        const out: Record<string, string[]> = {}
        for (const lang of langs) {
            const values = fromLines(buffer[lang] ?? "")
            if (values.length) {
                out[lang] = values
            } else if (keepEmpty.has(lang)) {
                out[lang] = []
            }
        }
        return out
    }

    function rowsToSlots(rows: SlotRow[]): Record<string, Slot> {
        const out: Record<string, Slot> = {}
        for (const row of rows) {
            const name = row.name.trim()
            if (!name) continue
            out[name] = { entity: row.entity.trim(), context: fromCsv(row.contextText) }
        }
        return out
    }

    // rowsToSlots() builds a Record, so a duplicate name silently overwrites the
    // other slot's entity and context and a blank one vanishes. Rust cannot
    // catch either - by the time it sees the data it is already a HashMap - so
    // the check has to live here, before the save.
    function slotRowProblem(rows: SlotRow[]): string {
        const seen = new Set<string>()

        for (const row of rows) {
            const name = row.name.trim()

            if (!name) return t('commands-slot-error-empty')
            if (seen.has(name)) return `${t('commands-slot-error-duplicate')} "${name}"`

            seen.add(name)
        }

        return ""
    }

    function projectPack(
        p: CommandPack | null,
        c: JCommand | null,
        index: number,
        phrases: Record<string, string>,
        sounds: Record<string, string>,
        exeArgs: string,
        cliArgs: string,
        slots: SlotRow[],
        langs: string[],
        keepEmptyPhrases: Set<string>,
        keepEmptySounds: Set<string>
    ): CommandPack | null {
        if (!p) return null
        if (!c || index < 0 || index >= p.commands.length) return p

        const commands = p.commands.slice()
        commands[index] = {
            ...c,
            exe_args: fromArgLines(exeArgs),
            cli_args: fromArgLines(cliArgs),
            phrases: linesToMap(phrases, langs, keepEmptyPhrases),
            sounds: linesToMap(sounds, langs, keepEmptySounds),
            slots: rowsToSlots(slots),
            timeout: Number.isFinite(Number(c.timeout)) ? Number(c.timeout) : 0
        }

        return { ...p, commands }
    }

    // re-derives from pack + cmdIndex rather than reading the reactive
    // `projected`, so it is correct even when called in the same tick as a
    // change to cmdIndex, before Svelte has flushed.
    //
    // returns false when the form cannot be folded into the draft without
    // losing something - the caller must then abort, because this is the point
    // where rowsToSlots() would collapse two same-named slots into one.
    function commitForm(): boolean {
        if (!pack || cmdIndex < 0 || cmdIndex >= pack.commands.length) return true

        const slotProblem = slotRowProblem(slotRows)
        if (slotProblem) {
            fail('commands-validation-title', slotProblem)
            return false
        }

        const next = projectPack(pack, pack.commands[cmdIndex], cmdIndex, phraseText,
                                 soundText, exeArgsText, cliArgsText, slotRows, formLangs,
                                 emptyPhraseLangs, emptySoundLangs)
        if (next) pack = next

        return true
    }

    function errText(err: unknown): string {
        // Tauri rejects with a plain String
        return typeof err === "string" ? err : String(err)
    }

    function clearBanners() {
        savedMsg = ""
        errorMsg = ""
        errorTitle = ""
        validationErrors = []
        warnings = []
        reloadNote = ""
        reloadSkipped = []
    }

    // one entry point for the red banner, so the title always names what
    // actually failed.
    //
    // it clears savedMsg by default: "Command pack saved" next to an error is
    // not a state the user can read. keepSaved is for the hot-reload branch,
    // where the disk write really did succeed and the two banners say different
    // true things - "Command pack deleted" and "The assistant did not apply the
    // change" are not a contradiction.
    function fail(titleKey: string, message: string, keepSaved = false) {
        if (!keepSaved) savedMsg = ""
        errorTitle = t(titleKey)
        errorMsg = message
    }

    // ### LOADING
    async function loadStatics() {
        try {
            const [types, levels, timeout] = await Promise.all([
                invoke<string[]>("get_command_types"),
                invoke<string[]>("get_sandbox_levels"),
                invoke<number>("get_default_timeout")
            ])
            commandTypes = types
            sandboxLevels = levels
            defaultTimeout = timeout
        } catch (err) {
            console.error("failed to load command editor statics:", err)
            commandTypes = ["voice", "lua", "ahk", "cli", "terminate", "stop_chaining"]
            sandboxLevels = ["minimal", "standard", "full"]
        }
    }

    async function loadPacks() {
        listError = ""
        try {
            packs = await invoke<CommandPack[]>("list_command_packs")
        } catch (err) {
            console.error("failed to list command packs:", err)
            packs = []
            listError = errText(err)
        } finally {
            loadingPacks = false
        }
    }

    async function loadSoundNames(langs: string[]) {
        const names: Record<string, string[]> = {}
        for (const lang of langs) {
            try {
                names[lang] = await invoke<string[]>("list_sound_names", { voiceId: "", lang })
            } catch {
                names[lang] = []
            }
        }
        soundNames = names
    }

    // re-read everything about the open pack from disk. also the tail of every
    // successful write, so the editor never serves its own pre-edit state.
    async function refreshOpenPack(name: string, keepIndex = -1) {
        const fresh = await invoke<CommandPack>("read_command_pack", { pack: name })

        try {
            packFiles = await invoke<PackFiles>("list_pack_files", { pack: name })
        } catch (err) {
            console.error("failed to list pack files:", err)
            packFiles = { scripts: [], executables: [] }
        }

        try {
            rawText = await invoke<string>("read_command_pack_raw", { pack: name })
        } catch (err) {
            console.error("failed to read raw command.toml:", err)
            rawText = ""
        }
        rawOriginal = rawText

        adoptPack(fresh, keepIndex)
    }

    function adoptPack(fresh: CommandPack, keepIndex = -1) {
        pack = fresh
        packOriginal = canon(fresh)

        if (keepIndex >= 0 && keepIndex < fresh.commands.length) {
            openCommand(keepIndex)
        } else {
            cmdIndex = -1
        }
    }

    // ### NAVIGATION
    async function openPack(name: string) {
        clearBanners()
        try {
            await refreshOpenPack(name)
            // a pack that does not parse is raw-only: the structured editor has
            // nothing to show and a structured save would clobber it unseen
            rawMode = pack !== null && pack.error !== null
            await loadSoundNames(LANGS)
            view = "pack"
        } catch (err) {
            console.error("failed to open command pack:", err)
            fail('commands-open-failed', errText(err))
        }
    }

    function closePack() {
        pack = null
        packOriginal = ""
        cmdIndex = -1
        rawMode = false
        rawText = ""
        rawOriginal = ""
        view = "packs"
        loadPacks()
    }

    function leavePack() {
        if (dirty) {
            modalKind = "discard"
            modalTo = "packs"
            return
        }
        closePack()
    }

    function openCommand(index: number) {
        if (!pack) return

        const c = pack.commands[index]

        // a lua command that omits timeout/sandbox gets them filled in here and
        // the dirty BASELINE is re-taken, so opening it shows usable values
        // without claiming the user edited anything. re-taking the baseline is
        // the whole point: as a reactive block this same fill lit "Unsaved
        // changes" and raised a discard prompt on a command nobody had touched.
        //
        // only when the draft was clean, though - re-baselining a pack that
        // already has unsaved edits would swallow THOSE, and the discard guard
        // would then let the user walk away from them.
        const wasClean = canon(pack) === packOriginal

        if (applyLuaDefaults(c)) {
            pack = pack
            if (wasClean) packOriginal = canon(pack)
        }

        // a language the pack carries but SUPPORTED_LANGUAGES does not know
        // still gets a textarea, so saving cannot drop it
        formLangs = Array.from(new Set([
            ...LANGS,
            ...Object.keys(c.phrases ?? {}),
            ...Object.keys(c.sounds ?? {})
        ]))

        const phrases: Record<string, string> = {}
        const sounds: Record<string, string> = {}
        for (const lang of formLangs) {
            phrases[lang] = toLines(c.phrases ? c.phrases[lang] : [])
            sounds[lang] = toLines(c.sounds ? c.sounds[lang] : [])
        }
        phraseText = phrases
        soundText = sounds

        emptyPhraseLangs = emptyLangsOf(c.phrases)
        emptySoundLangs = emptyLangsOf(c.sounds)

        exeArgsText = toLines(c.exe_args)
        cliArgsText = toLines(c.cli_args)

        slotRows = Object.entries(c.slots ?? {}).map(([name, slot]) => ({
            name,
            entity: slot.entity ?? "",
            contextText: (slot.context ?? []).join(", ")
        }))

        cmdIndex = index
        loadSoundNames(formLangs)
    }

    function emptyLangsOf(map?: Record<string, string[]>): Set<string> {
        return new Set(Object.entries(map ?? {})
            .filter(([, values]) => (values ?? []).length === 0)
            .map(([lang]) => lang))
    }

    // a lua command with no timeout dies on its first VM hook with "Script
    // timeout" (and validate_pack refuses to write one), and an empty sandbox
    // leaves the picker showing option[0] while the value stays "".
    //
    // returns true when it changed something, so the caller can decide whether
    // that counts as a user edit. it does on a type change; it must not on a
    // plain open, which is what openCommand() re-baselines for.
    function applyLuaDefaults(c: JCommand): boolean {
        if (c.type !== "lua") return false

        let changed = false

        if (!c.timeout || c.timeout < 1) {
            c.timeout = defaultTimeout || 10000
            changed = true
        }
        if (!c.sandbox) {
            c.sandbox = "standard"
            changed = true
        }

        return changed
    }

    // NativeSelect binds cmd.type before this runs, so `cmd` already holds the
    // new value. deliberately one-way: the defaults are NOT stripped when the
    // type moves away from "lua", because that would throw away a timeout the
    // user set by hand on one stray change of a picker.
    function onTypeChange() {
        if (!cmd) return

        applyLuaDefaults(cmd)
        cmd = cmd
    }

    function enterCommand(index: number) {
        openCommand(index)
        view = "command"
    }

    // leaving the form only commits the draft - nothing is lost, so there is
    // nothing to confirm. the discard prompt belongs to leaving the PACK.
    function leaveForm() {
        if (!commitForm()) return

        cmdIndex = -1
        view = "pack"
    }

    function newCommand() {
        if (!pack) return
        if (!commitForm()) return
        if (!pack) return

        // starts as "voice", so no lua default applies yet; onTypeChange() fills
        // them in the moment the user picks "lua"
        pack.commands = [...pack.commands, {
            id: "",
            type: "voice",
            description: "",
            exe_path: "",
            exe_args: [],
            cli_cmd: "",
            cli_args: [],
            script: "",
            sandbox: "",
            timeout: 0,
            sounds: {},
            phrases: {},
            slots: {}
        }]
        pack = pack

        enterCommand(pack.commands.length - 1)
    }

    function addSlot() {
        slotRows = [...slotRows, { name: "", entity: "", contextText: "" }]
    }

    function removeSlot(index: number) {
        slotRows = slotRows.filter((_, i) => i !== index)
    }

    // ### MODAL
    function closeModal() {
        modalKind = ""
        modalName = ""
        modalPack = ""
        modalTyped = ""
        modalIndex = -1
    }

    function askCreatePack() {
        closeModal()
        modalKind = "createPack"
    }

    function askDeletePack(name: string) {
        closeModal()
        modalKind = "deletePack"
        modalPack = name
    }

    function askDeleteCommand(index: number) {
        closeModal()
        modalKind = "deleteCommand"
        modalIndex = index
    }

    // in-memory only: the command leaves the draft, disk is untouched until save
    function performDeleteCommand() {
        const index = modalIndex
        closeModal()

        if (!pack || index < 0) return

        if (!commitForm()) return
        if (!pack) return

        pack.commands = pack.commands.filter((_, i) => i !== index)
        pack = pack

        if (cmdIndex === index) {
            cmdIndex = -1
            view = "pack"
        } else if (cmdIndex > index) {
            cmdIndex -= 1
        }
    }

    function performDiscard() {
        const to = modalTo
        closeModal()

        pack = null
        packOriginal = ""
        cmdIndex = -1
        rawMode = false
        rawText = ""
        rawOriginal = ""

        if (to === "url") {
            const url = pendingUrl || "/"
            pendingUrl = ""
            $goto(url)
            return
        }

        view = "packs"
        loadPacks()
    }

    // ### WRITES
    async function performCreatePack() {
        const name = modalName.trim()
        closeModal()
        clearBanners()

        saving = true
        try {
            const fresh = await invoke<CommandPack>("create_command_pack", { pack: name })
            await loadPacks()
            refreshCommandsCount()
            await openPack(fresh.name)
        } catch (err) {
            console.error("failed to create command pack:", err)
            fail('commands-create-failed', errText(err))
            saving = false
            return
        }

        await applyHotReload()
        saving = false
    }

    async function performDeletePack() {
        const name = modalPack
        const typed = modalTyped
        closeModal()
        clearBanners()

        saving = true
        try {
            await invoke("delete_command_pack", { pack: name, confirm: typed })

            if (pack && pack.name === name) {
                pack = null
                packOriginal = ""
                cmdIndex = -1
                rawMode = false
                rawText = ""
                rawOriginal = ""
            }
            view = "packs"

            await loadPacks()
            refreshCommandsCount()

            savedMsg = t('commands-deleted')
            setTimeout(() => { savedMsg = "" }, 5000)
        } catch (err) {
            console.error("failed to delete command pack:", err)
            fail('commands-delete-failed', errText(err))
            saving = false
            return
        }

        await applyHotReload()
        saving = false
    }

    async function savePack() {
        if (!pack) return

        // also runs the slot-name check; a duplicate name would be collapsed
        // here, before anything is sent
        if (!commitForm()) return
        if (!pack) return

        const target = pack
        const keepIndex = view === "command" ? cmdIndex : -1

        // clearBanners() would wipe the message commitForm() just set, so it
        // runs only once the form is known to be committable
        clearBanners()
        saving = true

        // the raw buffer is not part of what this save writes, and
        // refreshOpenPack() below resets it from disk. losing an unsaved draft
        // to a save the user made in the OTHER tab is not something to do
        // quietly, so it is refused instead.
        if (rawDirty) {
            fail('commands-validation-title', t('commands-other-buffer-raw'))
            saving = false
            return
        }

        // cheap local pass first: these never need a round trip, and catching
        // them here keeps a half-typed command out of the log
        const ids = target.commands.map(c => (c.id ?? "").trim())

        const emptyId = ids.findIndex(id => !id)
        if (emptyId >= 0) {
            fail('commands-validation-title', `commands[${emptyId}].id: must not be empty`)
            saving = false
            return
        }

        const dupeId = ids.findIndex((id, i) => ids.indexOf(id) !== i)
        if (dupeId >= 0) {
            fail('commands-validation-title',
                 `commands[${dupeId}].id '${ids[dupeId]}': duplicated inside pack '${target.name}'`)
            saving = false
            return
        }

        const noType = target.commands.findIndex(c => !c.type)
        if (noType >= 0) {
            fail('commands-validation-title', `commands[${noType}].type: must not be empty`)
            saving = false
            return
        }

        // never throws by design, so it cannot block the save on its own
        try {
            const report = await invoke<PackValidation>("validate_command_pack",
                { pack: target.name, commands: target.commands })
            validationErrors = report.errors
            warnings = report.warnings
        } catch (err) {
            console.error("failed to validate command pack:", err)
            validationErrors = []
            warnings = []
        }

        try {
            await invoke<CommandPack>("save_command_pack",
                { pack: target.name, commands: target.commands, revision: target.revision })

            await refreshOpenPack(target.name, keepIndex)
            await loadPacks()
            refreshCommandsCount()

            savedMsg = t('commands-saved')
            setTimeout(() => { savedMsg = "" }, 5000)
        } catch (err) {
            console.error("failed to save command pack:", err)
            // the red notification is the single message on a rejection - the
            // lists below would just repeat the same line
            validationErrors = []
            warnings = []
            fail('commands-validation-title', errText(err))
            saving = false
            return
        }

        await applyHotReload()
        saving = false
    }

    async function saveRaw() {
        if (!pack) return

        const target = pack
        clearBanners()
        saving = true

        // mirror of the guard in savePack(): this write comes from the raw
        // buffer and refreshOpenPack() then rebuilds the structured draft from
        // disk, so an unsaved structured edit would disappear without a word
        if (structDirty) {
            fail('commands-validation-title', t('commands-other-buffer-struct'))
            saving = false
            return
        }

        try {
            await invoke<CommandPack>("save_command_pack_raw",
                { pack: target.name, content: rawText, revision: target.revision })

            await refreshOpenPack(target.name)
            await loadPacks()
            refreshCommandsCount()

            savedMsg = t('commands-saved')
            setTimeout(() => { savedMsg = "" }, 5000)

            // raw mode skips the filesystem half of validation - a missing .lua
            // file must not block repairing a broken pack - so those come back
            // here. the structural invariants were enforced before the write.
            if (pack) {
                try {
                    const report = await invoke<PackValidation>("validate_command_pack",
                        { pack: target.name, commands: pack.commands })
                    validationErrors = report.errors
                    warnings = report.warnings
                } catch {
                    validationErrors = []
                    warnings = []
                }
            }
        } catch (err) {
            console.error("failed to save raw command.toml:", err)
            fail('commands-validation-title', errText(err))
            saving = false
            return
        }

        await applyHotReload()
        saving = false
    }

    // ### HOT RELOAD
    //
    // three outcomes worth distinguishing, and the old code collapsed them:
    //   - ok:false  -> nothing was published, the assistant kept the old list
    //   - ok:true + retrainError -> the commands ARE live, the intent
    //     classifier could not be rebuilt, so phrase matching is stale
    //   - ok:true + skipped -> live, but packs whose TOML does not parse were
    //     dropped out of the assistant entirely
    async function applyHotReload() {
        if (!$ipcConnected) {
            // enableIpc(), not connectIpc(): a stop seen on another page latches
            // manualDisconnect, and only enableIpc() clears it
            enableIpc()
            await waitForIpcConnected(1500)
        }

        // returns the id the answering event will echo, or null on a closed
        // socket - sendAction() is fire-and-forget
        const requestId = reloadCommands()
        if (!requestId) {
            reloadOk = false
            reloadNote = t("commands-reload-offline")
            return
        }

        reloading = true
        const result = await awaitReload(requestId, 15000)
        reloading = false

        if (result === null) {
            reloadOk = false
            reloadNote = t("commands-reload-timeout")
            return
        }

        if (!result.ok) {
            // reload_all() only returns Err before the swap, so this really does
            // mean the previous commands are still the live ones
            reloadNote = ""
            fail('commands-reload-title', result.error || t("commands-reload-failed"), true)
            return
        }

        // live from here on - anything below is a caveat, never a refusal
        reloadSkipped = result.skipped

        if (result.retrainError) {
            reloadOk = false
            reloadNote = t("commands-reload-stale") + " " + result.retrainError
            return
        }

        reloadOk = reloadSkipped.length === 0
        reloadNote = t("commands-reload-ok")
            + ` (${result.packs}/${result.commands})`
            + (result.retrained ? " " + t("commands-reload-retrained") : "")
    }

    // ### INIT
    $beforeUrlChange(({ route }) => {
        if (!dirty) return true

        pendingUrl = route.url || route.sourceUrl.pathname || "/"
        modalKind = "discard"
        modalTo = "url"

        return false
    })

    onMount(async () => {
        // the page needs a live socket to confirm a hot reload. teardown stays
        // with App.svelte, which owns the connection for the whole window.
        enableIpc()

        await loadStatics()
        await loadPacks()
    })
</script>

<Space h="xl" />

{#if savedMsg}
    <Notification
        title={savedMsg}
        icon={Check}
        color="teal"
        on:close={() => { savedMsg = "" }}
    />
    <Space h="md" />
{/if}

{#if errorMsg}
    <Notification
        title={errorTitle}
        icon={CrossCircled}
        color="red"
        on:close={() => { errorMsg = "" }}
    >
        {errorMsg}
    </Notification>
    <Space h="md" />
{/if}

{#if reloadNote}
    <Alert
        title={reloadNote}
        color={reloadOk ? "teal" : "orange"}
        variant="outline"
        withCloseButton
        on:close={() => { reloadNote = "" }}
    >
        {#if reloadSkipped.length}
            <Text size="xs" color="gray">
                {t('commands-reload-skipped')} {reloadSkipped.join(", ")}
            </Text>
        {/if}
    </Alert>
    <Space h="md" />
{/if}

{#if reloading}
    <Group spacing="xs">
        <Loader size="sm" />
        <Text size="sm" color="gray">{t('commands-reload-pending')}</Text>
    </Group>
    <Space h="md" />
{/if}

<!-- hard errors, kept apart from the warnings below: these are broken at
     runtime, not advisory. raw mode can still produce them, because it skips
     the checks that need the pack directory. -->
{#if validationErrors.length}
    <Alert
        title={t('commands-errors-title')}
        icon={CrossCircled}
        color="red"
        variant="outline"
    >
        {#each validationErrors as problem}
            <Text size="xs" color="gray">{problem}</Text>
        {/each}
    </Alert>
    <Space h="md" />
{/if}

{#if warnings.length}
    <Alert
        title={t('commands-warnings-title')}
        icon={ExclamationTriangle}
        color="orange"
        variant="outline"
    >
        {#each warnings as warning}
            <Text size="xs" color="gray">{warning}</Text>
        {/each}
    </Alert>
    <Space h="md" />
{/if}

<!-- ### VIEW 1 - the pack list -->
{#if view === "packs"}
    <Group position="apart">
        <Text size="lg" weight={600}>{t('commands-packs')}</Text>
        <Group spacing="xs">
            <ActionIcon color="gray" title={t('commands-pack-new')} on:click={askCreatePack}>
                <Plus />
            </ActionIcon>
            <ActionIcon color="gray" on:click={loadPacks}>
                <Reload />
            </ActionIcon>
        </Group>
    </Group>

    <Space h="sm" />

    <Input
        icon={MagnifyingGlass}
        placeholder={t('commands-search')}
        variant="filled"
        bind:value={packFilter}
    />

    <Space h="md" />

    {#if listError}
        <Alert title={t('notification-error')} color="red" variant="outline">
            <Text size="sm" color="gray">{listError}</Text>
        </Alert>
        <Space h="sm" />
    {/if}

    {#if loadingPacks}
        <Group spacing="xs">
            <Loader size="sm" />
            <Text size="sm" color="gray">{t('commands-loading')}</Text>
        </Group>
    {:else}
        {#each filteredPacks as p (p.name)}
            <Paper shadow="xs" padding="md" withBorder>
                <Group position="apart" noWrap>
                    <button
                        class="row-hit"
                        type="button"
                        disabled={!p.managed}
                        on:click={() => openPack(p.name)}
                    >
                        <span class="row-title">{p.name}</span>
                        <span class="row-sub">{p.commands.length}</span>
                    </button>
                    <Group spacing="xs" noWrap>
                        {#if p.error}
                            <Badge color="red" variant="light">TOML</Badge>
                        {/if}
                        {#if !p.managed}
                            <Badge color="gray" variant="light">RO</Badge>
                        {/if}
                        <ActionIcon
                            color="gray"
                            title={t('commands-open-folder')}
                            on:click={() => showInExplorer(p.path)}
                        >
                            <InfoCircled />
                        </ActionIcon>
                        <ActionIcon
                            color="red"
                            title={t('commands-pack-delete')}
                            disabled={!p.managed}
                            on:click={() => askDeletePack(p.name)}
                        >
                            <Trash />
                        </ActionIcon>
                    </Group>
                </Group>
                {#if p.error}
                    <Space h="xs" />
                    <Text size="xs" color="red">{p.error}</Text>
                {/if}
                <!-- the assistant loads this pack fine; only the editor cannot
                     address a folder name outside its whitelist -->
                {#if !p.managed}
                    <Space h="xs" />
                    <Text size="xs" color="gray">{t('commands-pack-unmanaged')}</Text>
                {/if}
            </Paper>
            <Space h="xs" />
        {:else}
            <Alert title={t('commands-packs-empty')} color="orange" variant="outline" />
        {/each}
    {/if}
{/if}

<!-- ### VIEW 2 - one pack -->
{#if view === "pack" && pack}
    <Group position="apart" noWrap>
        <Text size="lg" weight={600}>{pack.name}</Text>
        <Group spacing="xs" noWrap>
            {#if dirty}
                <Badge color="orange" variant="light">{t('commands-unsaved')}</Badge>
            {/if}
            <ActionIcon
                color="gray"
                title={t('commands-command-new')}
                disabled={rawMode}
                on:click={newCommand}
            >
                <Plus />
            </ActionIcon>
            <ActionIcon
                color="red"
                title={t('commands-pack-delete')}
                on:click={() => askDeletePack(pack ? pack.name : "")}
            >
                <Trash />
            </ActionIcon>
        </Group>
    </Group>

    <Space h="sm" />

    <Switch label={t('commands-raw')} bind:checked={rawMode} disabled={rawLocked} />

    {#if pack.error}
        <Space h="sm" />
        <Alert title={t('commands-pack-broken')} color="red" variant="outline">
            <Text size="xs" color="gray">{pack.error}</Text>
        </Alert>
    {/if}

    <Space h="md" />

    {#if rawMode}
        <Text size="xs" color="gray">{t('commands-raw-desc')}</Text>
        <Space h="xs" />
        <Textarea rows={18} variant="filled" bind:value={rawText} />
        <Space h="md" />
        <Button
            color="lime"
            radius="md"
            size="sm"
            uppercase
            ripple
            fullSize
            disabled={saving || !rawDirty}
            on:click={saveRaw}
        >
            {t('settings-save')}
        </Button>
    {:else}
        {#each pack.commands as c, i (i)}
            <Paper shadow="xs" padding="md" withBorder>
                <Group position="apart" noWrap>
                    <button class="row-hit" type="button" on:click={() => enterCommand(i)}>
                        <span class="row-title">{c.id || t('commands-command-new')}</span>
                        <span class="row-sub">
                            {typeLabel(c.type)} &middot; {Object.values(c.phrases ?? {}).flat().length}
                        </span>
                    </button>
                    <ActionIcon
                        color="red"
                        title={t('commands-command-delete')}
                        on:click={() => askDeleteCommand(i)}
                    >
                        <Trash />
                    </ActionIcon>
                </Group>
            </Paper>
            <Space h="xs" />
        {:else}
            <Alert title={t('commands-pack-empty')} color="orange" variant="outline" />
            <Space h="xs" />
        {/each}

        <Space h="md" />

        <Text size="xs" color="gray">{t('commands-struct-desc')}</Text>
        <Space h="xs" />

        <Button
            color="lime"
            radius="md"
            size="sm"
            uppercase
            ripple
            fullSize
            disabled={saving || !structDirty}
            on:click={savePack}
        >
            {t('settings-save')}
        </Button>
    {/if}

    <Space h="sm" />

    <Button color="gray" radius="md" size="sm" uppercase fullSize on:click={leavePack}>
        {t('settings-back')}
    </Button>
{/if}

<!-- ### VIEW 3 - one command -->
{#if view === "command" && pack && cmd}
    <Group position="apart" noWrap>
        <Text size="lg" weight={600}>{cmd.id || t('commands-command-new')}</Text>
        <Group spacing="xs" noWrap>
            {#if dirty}
                <Badge color="orange" variant="light">{t('commands-unsaved')}</Badge>
            {/if}
            <ActionIcon
                color="red"
                title={t('commands-command-delete')}
                on:click={() => askDeleteCommand(cmdIndex)}
            >
                <Trash />
            </ActionIcon>
        </Group>
    </Group>

    <Space h="md" />

    <Accordion defaultValue="general">
        <Accordion.Item value="general">
            <div slot="control">{t('commands-section-general')}</div>

            <TextInput
                label={t('commands-field-id')}
                description={t('commands-field-id-desc')}
                variant="filled"
                required
                bind:value={cmd.id}
            />

            <Space h="sm" />

            {#key typeKey}
                <NativeSelect
                    data={typeSelectData}
                    label={t('commands-field-type')}
                    variant="filled"
                    bind:value={cmd.type}
                    on:change={onTypeChange}
                />
            {/key}

            <Space h="sm" />

            <Textarea
                label={t('commands-field-description')}
                rows={2}
                variant="filled"
                bind:value={cmd.description}
            />
        </Accordion.Item>

        <Accordion.Item value="exec">
            <div slot="control">{t('commands-section-exec')}</div>

            {#if cmd.type === "lua"}
                {#key scriptKey}
                    <NativeSelect
                        data={scriptData}
                        label={t('commands-field-script')}
                        description={t('commands-field-script-desc')}
                        variant="filled"
                        bind:value={cmd.script}
                    />
                {/key}

                <Space h="sm" />

                {#key sandboxKey}
                    <NativeSelect
                        data={sandboxSelectData}
                        label={t('commands-field-sandbox')}
                        variant="filled"
                        bind:value={cmd.sandbox}
                    />
                {/key}

                <Space h="sm" />

                <InputWrapper
                    label={t('commands-field-timeout')}
                    description={t('commands-field-timeout-desc')}
                >
                    <NumberInput min={100} max={600000} step={500} bind:value={cmd.timeout} />
                </InputWrapper>
            {:else if cmd.type === "ahk"}
                {#key exeKey}
                    <NativeSelect
                        data={exeData}
                        label={t('commands-field-exe')}
                        variant="filled"
                        bind:value={cmd.exe_path}
                    />
                {/key}

                <Space h="sm" />

                <Textarea
                    label={t('commands-field-args')}
                    description={t('commands-field-args-desc')}
                    rows={3}
                    variant="filled"
                    bind:value={exeArgsText}
                />
            {:else if cmd.type === "cli"}
                <TextInput
                    label={t('commands-field-cli')}
                    variant="filled"
                    bind:value={cmd.cli_cmd}
                />

                <Space h="sm" />

                <Textarea
                    label={t('commands-field-args')}
                    description={t('commands-field-args-desc')}
                    rows={3}
                    variant="filled"
                    bind:value={cliArgsText}
                />
            {:else}
                <Text size="sm" color="gray">{t('commands-no-exec-params')}</Text>
            {/if}
        </Accordion.Item>

        <Accordion.Item value="speech">
            <div slot="control">{t('commands-section-speech')}</div>

            {#each formLangs as lang (lang)}
                <Divider label={lang.toUpperCase()} labelPosition="left" />
                <Space h="xs" />

                <Textarea
                    label={t('commands-phrases')}
                    description={t('commands-phrases-desc')}
                    rows={4}
                    variant="filled"
                    bind:value={phraseText[lang]}
                />

                <Space h="xs" />

                <Textarea
                    label={t('commands-sounds')}
                    description={t('commands-sounds-desc')}
                    rows={2}
                    variant="filled"
                    bind:value={soundText[lang]}
                />

                <Space h="xs" />

                {#if (soundNames[lang] ?? []).length}
                    <Text size="xs" color="gray">
                        {t('commands-sounds-available')} {(soundNames[lang] ?? []).join(", ")}
                    </Text>
                {:else}
                    <Text size="xs" color="gray">{t('commands-sounds-none')}</Text>
                {/if}

                <Space h="md" />
            {/each}
        </Accordion.Item>

        <Accordion.Item value="slots">
            <div slot="control">{t('commands-section-slots')}</div>

            {#each slotRows as row, i (i)}
                <Paper padding="xs" withBorder>
                    <Group position="apart" noWrap align="flex-end">
                        <TextInput
                            label={t('commands-slot-name')}
                            variant="filled"
                            bind:value={row.name}
                        />
                        <ActionIcon color="red" on:click={() => removeSlot(i)}>
                            <Trash />
                        </ActionIcon>
                    </Group>

                    <Space h="xs" />

                    <TextInput
                        label={t('commands-slot-entity')}
                        description={t('commands-slot-entity-desc')}
                        variant="filled"
                        bind:value={row.entity}
                    />

                    <Space h="xs" />

                    <TextInput
                        label={t('commands-slot-context')}
                        description={t('commands-slot-context-desc')}
                        variant="filled"
                        bind:value={row.contextText}
                    />
                </Paper>
                <Space h="xs" />
            {/each}

            <Button color="gray" size="xs" uppercase on:click={addSlot}>
                {t('commands-slot-add')}
            </Button>
        </Accordion.Item>
    </Accordion>

    <Space h="md" />

    <Button
        color="lime"
        radius="md"
        size="sm"
        uppercase
        ripple
        fullSize
        disabled={saving || !structDirty}
        on:click={savePack}
    >
        {t('settings-save')}
    </Button>

    <Space h="sm" />

    <Button color="gray" radius="md" size="sm" uppercase fullSize on:click={leaveForm}>
        {t('settings-back')}
    </Button>
{/if}

<!--
    one Modal for every destructive or creating flow. @tauri-apps/plugin-dialog
    is deliberately not used: capabilities/default.json grants dialog:allow-message
    only, so ask()/confirm() would be denied at runtime.
-->
<Modal
    opened={modalKind !== ""}
    centered
    title={modalKind === "createPack" ? t('commands-pack-new')
         : modalKind === "deletePack" ? t('commands-pack-delete')
         : modalKind === "deleteCommand" ? t('commands-command-delete')
         : t('commands-discard')}
    on:close={closeModal}
>
    {#if modalKind === "createPack"}
        <TextInput
            label={t('commands-pack-name')}
            description={t('commands-pack-name-desc')}
            variant="filled"
            bind:value={modalName}
        />
        <Space h="md" />
        <Group position="right" spacing="xs">
            <Button color="gray" size="xs" uppercase on:click={closeModal}>
                {t('settings-cancel')}
            </Button>
            <Button
                color="lime"
                size="xs"
                uppercase
                disabled={!createNameOk}
                on:click={performCreatePack}
            >
                {t('commands-pack-new')}
            </Button>
        </Group>
    {:else if modalKind === "deletePack"}
        <Text size="sm" color="gray">{t('commands-pack-delete-confirm')}</Text>
        <Space h="sm" />
        <TextInput label={modalPack} variant="filled" bind:value={modalTyped} />
        <Space h="md" />
        <Group position="right" spacing="xs">
            <Button color="gray" size="xs" uppercase on:click={closeModal}>
                {t('settings-cancel')}
            </Button>
            <Button
                color="red"
                size="xs"
                uppercase
                disabled={modalTyped !== modalPack}
                on:click={performDeletePack}
            >
                {t('commands-delete')}
            </Button>
        </Group>
    {:else if modalKind === "deleteCommand"}
        <Text size="sm" color="gray">{t('commands-command-delete-confirm')}</Text>
        <Space h="md" />
        <Group position="right" spacing="xs">
            <Button color="gray" size="xs" uppercase on:click={closeModal}>
                {t('settings-cancel')}
            </Button>
            <Button color="red" size="xs" uppercase on:click={performDeleteCommand}>
                {t('commands-delete')}
            </Button>
        </Group>
    {:else if modalKind === "discard"}
        <Text size="sm" color="gray">{t('commands-discard-desc')}</Text>
        <Space h="md" />
        <Group position="right" spacing="xs">
            <Button color="gray" size="xs" uppercase on:click={closeModal}>
                {t('settings-cancel')}
            </Button>
            <Button color="red" size="xs" uppercase on:click={performDiscard}>
                {t('commands-discard-action')}
            </Button>
        </Group>
    {/if}
</Modal>

<HDivider />

<style lang="scss">
    .row-hit {
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: 0.15rem;
        flex: 1 1 auto;
        min-width: 0;
        padding: 0;
        background: transparent;
        border: none;
        color: inherit;
        text-align: left;
        cursor: pointer;
    }

    /* an unmanaged pack: listed and openable in Explorer, not editable here */
    .row-hit:disabled {
        cursor: default;
        opacity: 0.55;
    }

    .row-title {
        font-weight: 600;
        font-size: 0.85rem;
        color: #fff;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        max-width: 100%;
    }

    .row-sub {
        font-size: 0.7rem;
        color: rgba(255, 255, 255, 0.5);
    }
</style>
