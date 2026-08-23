<script lang="ts">
    import { onMount } from "svelte"
    import { invoke } from "@tauri-apps/api/core"
    import { goto } from "@roxi/routify"
    import { setTimeout } from "worker-timers"

    import { showInExplorer } from "@/functions"
    import { appInfo, assistantVoice, translations, translate, reloadSettings } from "@/stores"

    import HDivider from "@/components/elements/HDivider.svelte"

    import {
        Notification,
        Button,
        Text,
        Tabs,
        Space,
        Alert,
        TextInput,
        Textarea,
        NumberInput,
        PasswordInput,
        InputWrapper,
        NativeSelect,
        Switch
    } from "@svelteuidev/core"

    import {
        Check,
        Mix,
        Cube,
        Code,
        ChatBubble,
        Gear,
        QuestionMarkCircled,
        CrossCircled
    } from "radix-icons-svelte"

    $: t = (key: string) => translate($translations, key)

    interface VoiceMeta {
        id: string
        name: string
        author: string
        languages: string[]
    }

    interface VoiceConfig {
        voice: VoiceMeta
    }
    
    let availableVoices: VoiceMeta[] = []

    async function selectVoice(voiceId: string) {
        voiceVal = voiceId
        
        // play preview sound
        try {
            await invoke("preview_voice", { voiceId })
        } catch (err) {
            console.error("Failed to preview voice:", err)
        }
    }

    // ### STATE
    interface MicrophoneOption {
        label: string
        value: string
    }

    let availableMicrophones: MicrophoneOption[] = []
    let availableVoskModels: { label: string; value: string }[] = []
    let availableGlinerModels: { label: string; value: string }[] = []

    // shape returned by the list_backend_options command
    // (jarvis-core models/structs.rs BackendOption; note the snake_case fields)
    interface BackendOption {
        id: string
        name: string
        model_id: string | null
        is_default: boolean
    }

    let intentBackends: BackendOption[] = []
    let slotsBackends: BackendOption[] = []
    let vadBackends: BackendOption[] = []
    // the lists above are empty until the registry answers. without this flag
    // the "no slot backends" alert renders on every page open, before we know
    let backendsLoaded = false

    let settingsSaved = false
    let saveButtonDisabled = false
    let saveError = ""
    // extra line under the "saved" notification, when the save landed on disk
    // but could not be handed to a running assistant
    let saveNotice = ""

    // form values (state vars)
    let voiceVal = ""
    let selectedMicrophone = ""
    let selectedWakeWordEngine = ""
    let selectedIntentRecognitionEngine = ""
    let selectedSlotExtractionEngine = ""
    let selectedGlinerModel = ""
    let selectedVoskModel = ""
    let selectedNoiseSuppression = ""
    let selectedVad = ""
    let gainNormalizerEnabled = false
    let apiKeyOpenai = ""

    let llmEnabled = false
    let llmBaseUrl = ""
    let llmModel = ""
    let llmTimeout = 60
    let llmMaxTokens = 2048
    let llmSystemPrompt = ""
    let llmAllowRemote = false

    // mirrors is_loopback_url in crates/jarvis-core/src/db/structs.rs. purely an
    // early warning next to the field - the real gate is Settings::validate_change(),
    // which is what rejects the save, and llm::LlmConfig::from_settings(),
    // which is what refuses to send.
    //
    // it has to fail closed on exactly the same characters as the Rust side:
    // "http://evil.com\@127.0.0.1/v1" reads as host evil.com to any WHATWG
    // parser (a backslash is a path delimiter for http/https) and as host
    // 127.0.0.1 to a naive "text after the last @" split. tab/CR/LF are
    // stripped before parsing and non-ASCII goes through IDNA, so those are out
    // too - none of them belongs in a local endpoint address.
    const URL_UNSAFE_CHAR = /[\\\s\u0000-\u001f\u007f-\uffff]/

    function isLoopbackUrl(url: string): boolean {
        const trimmed = url.trim()
        if (URL_UNSAFE_CHAR.test(trimmed)) return false
        const m = /^https?:\/\/(?:[^@/]*@)?(\[[^\]]+\]|[^:/?#]+)/i.exec(trimmed)
        if (!m) return false
        const host = m[1].replace(/^\[|\]$/g, "").toLowerCase()
        if (host === "localhost" || host === "::1") return true
        const v4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(host)
        return !!v4 && v4.slice(1).every(o => +o <= 255) && +v4[1] === 127
    }

    // what the endpoint box saves. an empty box must NOT be sent: Settings::set
    // rejects "" and db_write_many is all-or-nothing, so one cleared field on
    // this tab would fail the save of every unrelated setting on every other
    // tab. same guard as llm_timeout below.
    const DEFAULT_LLM_BASE_URL = "http://127.0.0.1:1234/v1"
    $: llmBaseUrlToSave = llmBaseUrl.trim() || DEFAULT_LLM_BASE_URL

    // what the timeout box saves. a cleared NumberInput binds "" / undefined,
    // which would go out as "NaN"; a value stored before the floor moved to 10
    // would be rejected outright. Settings::set refuses both and db_write_many
    // is all-or-nothing, so either one would fail the whole form save.
    $: llmTimeoutToSave = Math.min(600, Math.max(10,
        Math.round(Number.isFinite(+llmTimeout) ? +llmTimeout : 60)))

    // a cleared NumberInput binds "" / undefined and would go out as "NaN";
    // Settings::set refuses it and db_write_many is all-or-nothing, so one
    // empty box here would fail the save of every setting on every tab
    $: llmMaxTokensToSave = Math.min(32768, Math.max(64,
        Math.round(Number.isFinite(+llmMaxTokens) ? +llmMaxTokens : 2048)))

    $: llmRemoteBlocked =
        llmBaseUrl.trim() !== "" && !isLoopbackUrl(llmBaseUrl) && !llmAllowRemote

    // {label,value} for NativeSelect. label prefers a `backend-<id>` translation
    // and falls back to the English name the registry reports (model.toml `name`
    // or catalog::code_backends). reactive on both the options and the language
    const toSelectData = (opts: BackendOption[], trans: Record<string, string>) =>
        opts.map(o => ({ label: translate(trans, `backend-${o.id}`, o.name), value: o.id }))

    $: intentSelectData = toSelectData(intentBackends, $translations)
    $: slotsSelectData = toSelectData(slotsBackends, $translations)
    $: vadSelectData = toSelectData(vadBackends, $translations)

    // subscribe to stores
    assistantVoice.subscribe(value => {
        voiceVal = value
    })

    let logFilePath = ""
    appInfo.subscribe(info => {
        logFilePath = info.logFilePath
    })

    // ### FUNCTIONS
    async function saveSettings() {
        saveButtonDisabled = true
        settingsSaved = false
        saveError = ""
        saveNotice = ""

        try {
            // one call, not twelve. db_write_many validates every entry before
            // it writes anything, so a rejected value cannot leave the db half
            // saved, and app.db is rewritten once instead of once per field
            await invoke("db_write_many", {
                entries: {
                    assistant_voice: voiceVal,
                    selected_microphone: selectedMicrophone,
                    selected_wake_word_engine: selectedWakeWordEngine,
                    intent_backend: selectedIntentRecognitionEngine,
                    slots_backend: selectedSlotExtractionEngine,
                    selected_gliner_model: selectedGlinerModel,
                    selected_vosk_model: selectedVoskModel,

                    noise_suppression: selectedNoiseSuppression,
                    vad_backend: selectedVad,
                    gain_normalizer: gainNormalizerEnabled.toString(),

                    api_key__openai: apiKeyOpenai,

                    llm_enabled: llmEnabled.toString(),
                    llm_base_url: llmBaseUrlToSave,
                    llm_model: llmModel.trim(),
                    llm_timeout: llmTimeoutToSave.toString(),
                    llm_max_tokens: llmMaxTokensToSave.toString(),
                    llm_system_prompt: llmSystemPrompt,
                    llm_allow_remote: llmAllowRemote.toString()
                }
            })

            // the boxes may have been empty or out of range - show what was
            // actually stored
            llmBaseUrl = llmBaseUrlToSave
            llmTimeout = llmTimeoutToSave
            llmMaxTokens = llmMaxTokensToSave

            // jarvis-app read app.db once, at startup, and this is a different
            // process: without this action nothing saved here reaches the
            // running assistant and the LLM tab appears to do nothing. it
            // adopts the llm_* keys only; everything else was consumed at init
            // and still needs a restart.
            saveNotice = reloadSettings() ? "" : t('settings-saved-restart-hint')

            // update shared store
            assistantVoice.set(voiceVal)
            settingsSaved = true

            // hide alert after 5 seconds
            setTimeout(() => {
                settingsSaved = false
            }, 5000)

            // restart listening with new settings
            // stopListening(() => startListening())
        } catch (err) {
            console.error("failed to save settings:", err)
            saveError = typeof err === "string" ? err : String(err)
        }

        setTimeout(() => {
            saveButtonDisabled = false
        }, 1000)
    }

    // ### INIT
    onMount(async () => {
        // backend options first: everything below this is slower (voice
        // scanning, and pv_get_audio_devices in particular), and until these
        // land the three backend selects have no options and the slots alert
        // would claim nothing is installed.
        // own try/catch: this must never abort the settings load below
        try {
            const [intentOpts, slotsOpts, vadOpts] = await Promise.all([
                invoke<BackendOption[]>("list_backend_options", { task: "intent" }),
                invoke<BackendOption[]>("list_backend_options", { task: "slots" }),
                invoke<BackendOption[]>("list_backend_options", { task: "vad" })
            ])
            intentBackends = intentOpts
            slotsBackends = slotsOpts
            vadBackends = vadOpts
        } catch (err) {
            console.error("Failed to load backend options:", err)
            intentBackends = []
            slotsBackends = []
            vadBackends = []
        }
        backendsLoaded = true

        // load voices
        try {
            const voices = await invoke<VoiceConfig[]>("list_voices")
            availableVoices = voices.map(v => v.voice)
        } catch (err) {
            console.error("Failed to load voices:", err)
            availableVoices = []
        }

        try {
            // load microphones
            const mics = await invoke<string[]>("pv_get_audio_devices")
            availableMicrophones = [
                { label: t('settings-mic-default'), value: "-1" },  // system default
                ...mics.map((name, idx) => ({
                    label: name,
                    value: String(idx)
                }))
            ]

            // load vosk models
            const languageNames: Record<string, string> = {
                us: 'English',
                ru: 'Русский',
                uk: 'Українська',
                de: 'German',
                fr: 'French',
                es: 'Spanish',
                // ..
            };
            const voskModels = await invoke<{ name: string; language: string; size: string }[]>("list_vosk_models")
            availableVoskModels = voskModels.map(m => ({
                label: `${m.name} (${languageNames[m.language] ?? m.language}, ${m.size})`,
                value: m.name
            }))

            // load gliner models
            const glinerModels = await invoke<{ display_name: string; value: string }[]>("list_gliner_models")
            availableGlinerModels = glinerModels.map(m => ({
                label: m.display_name,
                value: m.value,
            }))

            // load settings from db
            const [mic, wakeWord, intentReco, slotEngine, glinerModel, voskModel,
                   noiseSuppression, vad, gainNormalizer,
                   openai,
                   llmEnabledRaw, llmBaseUrlRaw, llmModelRaw,
                   llmTimeoutRaw, llmMaxTokensRaw, llmSystemPromptRaw, llmAllowRemoteRaw] = await Promise.all([
                invoke<string>("db_read", { key: "selected_microphone" }),
                invoke<string>("db_read", { key: "selected_wake_word_engine" }),
                invoke<string>("db_read", { key: "intent_backend" }),
                invoke<string>("db_read", { key: "slots_backend" }),
                invoke<string>("db_read", { key: "selected_gliner_model" }),
                invoke<string>("db_read", { key: "selected_vosk_model" }),

                invoke<string>("db_read", { key: "noise_suppression" }),
                invoke<string>("db_read", { key: "vad_backend" }),
                invoke<string>("db_read", { key: "gain_normalizer" }),

                invoke<string>("db_read", { key: "api_key__openai" }),

                invoke<string>("db_read", { key: "llm_enabled" }),
                invoke<string>("db_read", { key: "llm_base_url" }),
                invoke<string>("db_read", { key: "llm_model" }),
                invoke<string>("db_read", { key: "llm_timeout" }),
                invoke<string>("db_read", { key: "llm_max_tokens" }),
                invoke<string>("db_read", { key: "llm_system_prompt" }),
                invoke<string>("db_read", { key: "llm_allow_remote" })
            ])

            selectedMicrophone = mic
            selectedWakeWordEngine = wakeWord
            selectedIntentRecognitionEngine = intentReco
            selectedSlotExtractionEngine = slotEngine
            selectedVoskModel = voskModel
            selectedGlinerModel = glinerModel
            selectedNoiseSuppression = noiseSuppression
            selectedVad = vad
            gainNormalizerEnabled = gainNormalizer === "true"
            apiKeyOpenai = openai

            llmEnabled = llmEnabledRaw === "true"
            llmBaseUrl = llmBaseUrlRaw
            llmModel = llmModelRaw
            // db_read returns "" for a key Settings::get does not know
            // (tauri_commands/db.rs), and parseInt("") is NaN, which a
            // NumberInput renders as an empty box that then saves as "NaN"
            llmTimeout = parseInt(llmTimeoutRaw) || 60
            llmMaxTokens = parseInt(llmMaxTokensRaw) || 2048
            llmSystemPrompt = llmSystemPromptRaw
            llmAllowRemote = llmAllowRemoteRaw === "true"

            // never hold a value that is not in its option list: NativeSelect
            // shows option[0] while the variable keeps the stale id (it renders
            // selected={item.value === value}), and Save writes the stale id back.
            //
            // falls back to the option the registry marks as default, NOT to
            // opts[0]: opts[0] is always "none", so clamping there would quietly
            // switch intent recognition off, while the Rust clamp
            // (Settings::sanitize_backends -> catalog::default_backend) would put
            // it on "intent-classifier". both sides now read the same value.
            const clamp = (val: string, opts: BackendOption[]) => {
                if (opts.length === 0 || opts.some(o => o.id === val)) return val
                const fallback = opts.find(o => o.is_default) ?? opts[0]
                console.warn(`backend '${val}' is not available, using '${fallback.id}'`)
                return fallback.id
            }

            selectedIntentRecognitionEngine = clamp(selectedIntentRecognitionEngine, intentBackends)
            selectedSlotExtractionEngine = clamp(selectedSlotExtractionEngine, slotsBackends)
            selectedVad = clamp(selectedVad, vadBackends)
        } catch (err) {
            console.error("failed to load settings:", err)
        }
    })
</script>

<Space h="xl" />

<Notification
    title={t('settings-beta-title')}
    icon={QuestionMarkCircled}
    color="blue"
    withCloseButton={false}
>
    {t('settings-beta-desc')}<br />
    <Space h="sm" />
    <Button
        color="gray"
        radius="md"
        size="xs"
        uppercase
        on:click={() => showInExplorer(logFilePath)}
    >
        {t('settings-open-logs')}
    </Button>
</Notification>

<Space h="xl" />

{#if settingsSaved}
    <Notification
        title={t('notification-saved')}
        icon={Check}
        color={saveNotice ? "yellow" : "teal"}
        on:close={() => { settingsSaved = false }}
    >
        {saveNotice}
    </Notification>
    <Space h="xl" />
{/if}

{#if saveError}
    <Notification
        title={t('notification-error')}
        icon={CrossCircled}
        color="red"
        on:close={() => { saveError = "" }}
    >
        {saveError}
    </Notification>
    <Space h="xl" />
{/if}

<Tabs class="form" color="#8AC832" position="left">
    <Tabs.Tab label={t('settings-general')} icon={Gear}>
        <Space h="sm" />
        <div class="voice-select">
            <label>{t('settings-voice')}</label>
            <p class="description">{t('settings-voice-desc')}</p>
            
            <div class="voice-options">
                {#each availableVoices as voice}
                    <button 
                        type="button"
                        class="voice-option"
                        class:selected={voiceVal === voice.id}
                        on:click={() => selectVoice(voice.id)}
                    >
                        <div class="voice-info">
                            <span class="voice-name">{voice.name}</span>
                            {#if voice.author}
                                <span class="voice-author">by {voice.author}</span>
                            {/if}
                        </div>
                        <div class="voice-languages">
                            {#each voice.languages as lang}
                                <img 
                                    src="/media/flags/{lang.toUpperCase()}.png" 
                                    alt={lang} 
                                    width="20" 
                                    title={lang}
                                />
                            {/each}
                        </div>
                    </button>
                {/each}
                
                {#if availableVoices.length === 0}
                    <p class="no-voices">{t('settings-no-voices')}</p>
                {/if}
            </div>
        </div>
    </Tabs.Tab>

    <Tabs.Tab label={t('settings-devices')} icon={Mix}>
        <Space h="sm" />
        <NativeSelect
            data={availableMicrophones}
            label={t('settings-microphone')}
            description={t('settings-microphone-desc')}
            variant="filled"
            bind:value={selectedMicrophone}
        />
    </Tabs.Tab>

    <Tabs.Tab label={t('settings-neural-networks')} icon={Cube}>
        <Space h="sm" />
        <!--
            values must be the WakeWordEngine variant names: db_read returns
            format!("{:?}", ..) so this is what comes back on load, and
            Settings::set lowercases before matching them.

            Porcupine was removed entirely - it had no implementation and left
            the assistant with no wake word at all when selected.
        -->
        <NativeSelect
            data={[
                { label: "Rustpotter", value: "Rustpotter" },
                { label: "Vosk", value: "Vosk" }
            ]}
            label={t('settings-wake-word-engine')}
            description={t('settings-wake-word-desc')}
            variant="filled"
            bind:value={selectedWakeWordEngine}
        />


        <Space h="xl" />
        {#key availableVoskModels}
        <NativeSelect
            data={[
                { label: t('settings-auto-detect'), value: "" },
                ...availableVoskModels
            ]}
            label={t('settings-vosk-model')}
            description={t('settings-vosk-model-desc')}
            variant="filled"
            bind:value={selectedVoskModel}
        />
        {/key}

        {#if availableVoskModels.length === 0}
            <Space h="sm" />
            <Alert title={t('settings-models-not-found')} color="orange" variant="outline">
                <Text size="sm" color="gray">
                    {t('settings-models-hint')}
                </Text>
            </Alert>
        {/if}

        <Space h="xl" />
        {#key intentSelectData}
        <NativeSelect
            data={intentSelectData}
            label={t('settings-intent-engine')}
            description={t('settings-intent-engine-desc')}
            variant="filled"
            bind:value={selectedIntentRecognitionEngine}
        />
        {/key}

        <Space h="xl" />
        {#key slotsSelectData}
        <NativeSelect
            data={slotsSelectData}
            label={t('settings-slot-engine')}
            description={t('settings-slot-engine-desc')}
            variant="filled"
            bind:value={selectedSlotExtractionEngine}
        />
        {/key}

        {#if backendsLoaded && slotsBackends.length <= 1}
            <Space h="sm" />
            <Alert title={t('settings-models-not-found')} color="orange" variant="outline">
                <Text size="sm" color="gray">
                    {t('settings-slots-no-backends')}
                </Text>
            </Alert>
        {/if}

        {#if selectedSlotExtractionEngine && selectedSlotExtractionEngine !== "none"}
            <Space h="sm" />
            {#key availableGlinerModels}
            <NativeSelect
                data={[
                    { label: t('settings-auto-detect'), value: "" },
                    ...availableGlinerModels
                ]}
                label={t('settings-gliner-model')}
                description={t('settings-gliner-model-desc')}
                variant="filled"
                bind:value={selectedGlinerModel}
            />
            {/key}

            {#if availableGlinerModels.length === 0}
                <Space h="sm" />
                <Alert title={t('settings-models-not-found')} color="orange" variant="outline">
                    <Text size="sm" color="gray">
                        {t('settings-gliner-models-hint')}
                    </Text>
                </Alert>
            {/if}
        {/if}

        <Space h="xl" />
        <NativeSelect
            data={[
                { label: t('settings-disabled'), value: "None" },
                { label: "Nnnoiseless", value: "Nnnoiseless" }
            ]}
            label={t('settings-noise-suppression')}
            description={t('settings-noise-suppression-desc')}
            variant="filled"
            bind:value={selectedNoiseSuppression}
        />

        <Space h="md" />

        {#key vadSelectData}
        <NativeSelect
            data={vadSelectData}
            label={t('settings-vad')}
            description={t('settings-vad-desc')}
            variant="filled"
            bind:value={selectedVad}
        />
        {/key}

        <Space h="md" />

        <InputWrapper label={t('settings-gain-normalizer')}>
            <Text size="sm" color="gray">
                {t('settings-gain-normalizer-desc')}
            </Text>
            <Space h="xs" />
            <Switch
                label={gainNormalizerEnabled ? t('settings-enabled') : t('settings-disabled')}
                bind:checked={gainNormalizerEnabled}
            />
        </InputWrapper>
    </Tabs.Tab>

    <Tabs.Tab label={t('settings-llm')} icon={ChatBubble}>
        <Space h="sm" />

        <InputWrapper label={t('settings-llm-enabled')}>
            <Text size="sm" color="gray">{t('settings-llm-enabled-desc')}</Text>
            <Space h="xs" />
            <Switch
                label={llmEnabled ? t('settings-enabled') : t('settings-disabled')}
                bind:checked={llmEnabled}
            />
        </InputWrapper>

        <Space h="md" />

        <TextInput
            label={t('settings-llm-base-url')}
            description={t('settings-llm-base-url-desc')}
            variant="filled"
            autocomplete="off"
            placeholder="http://127.0.0.1:1234/v1"
            error={llmRemoteBlocked ? t('settings-llm-remote-blocked') : ""}
            bind:value={llmBaseUrl}
        />

        <Space h="md" />

        <TextInput
            label={t('settings-llm-model')}
            description={t('settings-llm-model-desc')}
            variant="filled"
            autocomplete="off"
            bind:value={llmModel}
        />

        <Space h="md" />

        <InputWrapper label={t('settings-llm-timeout')}>
            <Text size="sm" color="gray">{t('settings-llm-timeout-desc')}</Text>
            <Space h="xs" />
            <NumberInput min={10} max={600} step={5} variant="filled" bind:value={llmTimeout} />
        </InputWrapper>

        <Space h="md" />

        <InputWrapper label={t('settings-llm-max-tokens')}>
            <Text size="sm" color="gray">{t('settings-llm-max-tokens-desc')}</Text>
            <Space h="xs" />
            <NumberInput min={64} max={32768} step={256} variant="filled" bind:value={llmMaxTokens} />
        </InputWrapper>

        <Space h="md" />

        <Textarea
            label={t('settings-llm-system-prompt')}
            description={t('settings-llm-system-prompt-desc')}
            variant="filled"
            rows={4}
            bind:value={llmSystemPrompt}
        />

        <Space h="md" />

        <InputWrapper label={t('settings-llm-allow-remote')}>
            <Text size="sm" color="gray">{t('settings-llm-allow-remote-desc')}</Text>
            <Space h="xs" />
            <Switch
                label={llmAllowRemote ? t('settings-enabled') : t('settings-disabled')}
                bind:checked={llmAllowRemote}
            />
        </InputWrapper>

        <Space h="xl" />

        <InputWrapper label={t('settings-openai-key')}>
            <Text size="sm" color="gray">{t('settings-openai-key-desc')}</Text>
            <Space h="sm" />
            <PasswordInput
                icon={Code}
                placeholder={t('settings-openai-key')}
                variant="filled"
                autocomplete="off"
                bind:value={apiKeyOpenai}
            />
        </InputWrapper>
    </Tabs.Tab>
</Tabs>

<Space h="xl" />

<Button
    color="lime"
    radius="md"
    size="sm"
    uppercase
    ripple
    fullSize
    on:click={saveSettings}
    disabled={saveButtonDisabled}
>
    {t('settings-save')}
</Button>

<Space h="sm" />

<Button
    color="gray"
    radius="md"
    size="sm"
    uppercase
    fullSize
    on:click={() => $goto("/")}
>
    {t('settings-back')}
</Button>

<HDivider />

<style lang="scss">
.voice-select {
    margin-bottom: 1rem;
    
    label {
        font-weight: 600;
        font-size: 0.9rem;
        color: #fff;
        display: block;
        margin-bottom: 0.25rem;
    }
    
    .description {
        font-size: 0.75rem;
        color: rgba(255,255,255,0.5);
        margin: 0 0 0.75rem;
        white-space: pre-line;
    }
}

$voice-item-height: 70px;
$voice-item-gap: 0.5rem;
$voice-max-visible: 3;

.voice-options {
    display: flex;
    flex-direction: column;
    gap: $voice-item-gap;
    max-height: $voice-item-height * $voice-max-visible;
    overflow-y: auto;
    
    &::-webkit-scrollbar {
        width: 6px;
    }
    
    &::-webkit-scrollbar-track {
        background: rgba(255, 255, 255, 0.05);
        border-radius: 3px;
    }
    
    &::-webkit-scrollbar-thumb {
        background: rgba(255, 255, 255, 0.2);
        border-radius: 3px;
        
        &:hover {
            background: rgba(255, 255, 255, 0.3);
        }
    }
}

.voice-option {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.75rem 1rem;
    background: rgba(30, 40, 45, 0.8);
    border: 1px solid rgba(255,255,255,0.1);
    border-radius: 8px;
    cursor: pointer;
    transition: all 0.2s ease;
    text-align: left;
    width: 100%;
    
    &:hover {
        background: rgba(40, 55, 60, 0.9);
        border-color: rgba(255,255,255,0.2);
    }
    
    &.selected {
        background: rgba(82, 254, 254, 0.1);
        border-color: rgba(82, 254, 254, 0.4);
    }
}

.voice-info {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.15rem;
}

.voice-name {
    font-size: 0.85rem;
    color: #fff;
    font-weight: 500;
}

.voice-author {
    font-size: 0.7rem;
    color: rgba(255,255,255,0.4);
}

.voice-languages {
    display: flex;
    gap: 0.35rem;
    
    img {
        opacity: 0.8;
        border-radius: 2px;
    }
}

.no-voices {
    font-size: 0.8rem;
    color: rgba(255,255,255,0.4);
    font-style: italic;
}
</style>