# ### APP INFO
app-name = JARVIS
app-description = Voice Assistant

# ### TRAY MENU
tray-restart = Restart
tray-settings = Settings
tray-exit = Exit
tray-tooltip = JARVIS - Voice Assistant
tray-language = Language
tray-voice = Voice
tray-wake-word = Wake Word Engine
tray-noise-suppression = Noise Suppression
tray-vad = Voice Activity Detection
tray-gain-normalizer = Gain Normalizer

# ### HEADER
header-commands = COMMANDS
header-settings = SETTINGS

# ### SEARCH
search-placeholder = Enter a command manually or say «Jarvis» ...

# ### MAIN PAGE
assistant-not-running = ASSISTANT NOT RUNNING
assistant-offline-hint = You can configure it without starting.
btn-start = START
btn-starting = STARTING...

# ### STATUS
status-disconnected = Disconnected
status-standby = Standby
status-listening = Listening...
status-processing = Processing...

# ### STATS
stats-microphone = MICROPHONE
stats-neural-networks = NEURAL NETWORKS
stats-resources = RESOURCES
stats-system-default = System Default
stats-not-selected = Not selected
stats-loading = Loading...

# ### FOOTER
footer-author = Project author
footer-telegram = Our Telegram channel
footer-github = Github repository
footer-support = Support the project on

# ### SETTINGS
settings-title = Settings
settings-general = General
settings-devices = Devices
settings-neural-networks = Neural Networks
settings-audio = Audio
settings-recognition = Recognition
settings-about = About
settings-language = Language
settings-microphone = Microphone
settings-microphone-desc = The assistant will listen to this microphone.
settings-mic-default = Default (System)
settings-voice = Assistant voice
settings-voice-desc =
    Not all commands work with all sound packs.
    Click to listen the preview of sound.
settings-wake-word-engine = Wake word engine
settings-wake-word-desc = Choose the engine for wake word recognition.
settings-stt-engine = Speech recognition
settings-intent-engine = Intent recognition
settings-intent-engine-desc = Select neural network for command recognition.
settings-noise-suppression = Noise suppression
settings-noise-suppression-desc = Reduces background noise. May negatively affect recognition.
settings-vad = Voice detection (VAD)
settings-vad-desc = Skips silence, saves CPU resources.
settings-gain-normalizer = Gain normalizer
settings-gain-normalizer-desc = Automatically adjusts volume level.
settings-api-keys = API Keys
settings-save = Save
settings-cancel = Cancel
settings-back = Back
settings-enabled = Enabled
settings-disabled = Disabled

# settings - beta notice
settings-beta-title = BETA version!
settings-beta-desc = Some features may not work correctly.
settings-beta-feedback = Report all bugs to
settings-beta-bot = our Telegram bot
settings-open-logs = Open logs folder

settings-attention = Attention!

# settings - vosk
settings-auto-detect = Auto-detect
settings-vosk-model = Speech recognition model (Vosk)
settings-vosk-model-desc =
    Select Vosk model for speech recognition.
    You can download models here: https://alphacephei.com/vosk/models
settings-models-not-found = Models not found
settings-models-hint = Place Vosk models in resources/vosk folder

# settings - openai
settings-openai-key = OpenAI Key
settings-openai-not-supported = ChatGPT is not currently supported. It will be added in future updates.

# ### COMMANDS PAGE
commands-search = Search commands...
commands-loading = Loading command packs...
commands-packs = Packs
commands-packs-empty = No command packs found.
commands-pack-new = New pack
commands-pack-name = Pack name
commands-pack-name-desc = Letters, digits, "_" and "-" only. This becomes the folder name under resources/commands.
commands-pack-empty = This pack has no commands yet.
commands-pack-broken = This pack could not be parsed. Only raw TOML editing is available.
commands-pack-unmanaged = The assistant loads this pack, but the editor cannot manage a folder with this name. Rename it to letters, digits, "_" and "-" to edit it here.
commands-pack-delete = Delete pack
commands-pack-delete-confirm = Type the pack name to confirm. The folder and its scripts are moved to resources/.trash and are not deleted automatically.
commands-open-folder = Open folder
commands-command-new = New command
commands-command-delete = Delete command
commands-command-delete-confirm = Remove this command from the pack? The change is applied when you save.
commands-delete = Delete
commands-section-general = General
commands-section-exec = Execution
commands-section-speech = Phrases and sounds
commands-section-slots = Slots
commands-field-id = Command ID
commands-field-id-desc = Unique across all packs. This is what the intent classifier learns.
commands-field-type = Type
commands-field-description = Description
commands-field-script = Lua script
commands-field-script-desc = A .lua file inside the pack folder. Script bodies are edited outside the app.
commands-field-sandbox = Sandbox
commands-field-timeout = Timeout, ms
commands-field-timeout-desc = Lua only. From 100 to 600000.
commands-field-exe = Executable or .ahk script
commands-field-cli = Shell command
commands-field-args = Arguments
commands-field-args-desc = One per line, passed verbatim. A blank line is an empty argument.
commands-no-exec-params = This type has no execution parameters.
commands-phrases = Phrases
commands-phrases-desc = One per line. Wrap a slot name in curly braces to mark a parameter.
commands-sounds = Sounds
commands-sounds-desc = One name per line, without extension. Resolved against the selected voice.
commands-sounds-available = Available for this voice:
commands-sounds-none = No sounds for this language in the selected voice.
commands-slot-name = Slot name
commands-slot-entity = Entity
commands-slot-entity-desc = A free-form description GLiNER matches semantically, e.g. city name.
commands-slot-context = Context words
commands-slot-context-desc = Comma separated.
commands-slot-add = Add slot
commands-slot-error-empty = A slot with no name cannot be saved. Name it or remove the row.
commands-slot-error-duplicate = Two slots share the same name:
commands-raw = Raw TOML
commands-raw-desc = Replaces the whole file. This is the only mode that preserves comments and hand formatting.
commands-struct-desc = Saving rewrites command.toml. Comments and unknown keys are not preserved - use Raw TOML to keep them.
commands-other-buffer-raw = The raw TOML tab has unsaved edits. Save or discard them first, otherwise this save would throw them away.
commands-other-buffer-struct = The structured editor has unsaved edits. Save or discard them first, otherwise this save would throw them away.
commands-saved = Command pack saved
commands-deleted = Command pack deleted
commands-unsaved = Unsaved changes
commands-discard = Discard changes?
commands-discard-desc = This pack has edits that were never written to disk. Leaving now throws them away.
commands-discard-action = Discard
commands-validation-title = The pack was not saved
commands-open-failed = The pack could not be opened
commands-create-failed = The pack was not created
commands-delete-failed = The pack was not deleted
commands-reload-title = The assistant did not apply the change
commands-errors-title = Errors
commands-warnings-title = Warnings
commands-reload-pending = Applying the changes to the running assistant...
commands-reload-ok = The changes are live
commands-reload-retrained = Phrases changed, intent recognition was retrained.
commands-reload-skipped = These packs do not parse and were dropped from the assistant:
commands-reload-stale = The commands are live, but intent recognition could not be rebuilt and still matches the old phrases:
commands-reload-offline = Saved to disk. The assistant is not running, so the changes apply at its next start.
commands-reload-timeout = Saved to disk. The assistant has not confirmed yet - a large phrase change can take a while to retrain.
commands-reload-failed = The assistant refused the reload
cmdtype-voice = Voice reply only
cmdtype-lua = Lua script
cmdtype-ahk = AutoHotkey
cmdtype-cli = Shell command
cmdtype-terminate = Quit the assistant
cmdtype-stop_chaining = Stop command chaining
commands-sandbox-minimal = Minimal
commands-sandbox-standard = Standard
commands-sandbox-full = Full

# ### ERRORS
error-generic = An error occurred
error-connection = Connection error
error-not-found = Not found

# ### NOTIFICATIONS
notification-saved = Settings saved!
notification-error = Error
notification-assistant-started = Assistant started
notification-assistant-stopped = Assistant stopped

# SLOTS EXTRACTION
settings-slot-engine = Slot extraction
settings-slot-engine-desc = Extract parameters from voice commands (e.g. city name, number).
settings-gliner-model = GLiNER ONNX model
settings-gliner-model-desc =
    Select model variant.
    Smaller quantized models (int8, uint8) are faster but less accurate.
settings-gliner-models-hint = No GLiNER models found.

# ETC
search-error-not-running = Assistant is not running
search-error-failed = Failed to execute command
settings-no-voices = No voices found

# BACKEND OPTION LABELS (fall back to the English name from the model registry
# when a key is absent - see frontend translate(..., `backend-${id}`, o.name))
backend-none = Disabled
backend-intent-classifier = Intent Classifier
backend-energy = Energy-based
backend-nnnoiseless = Nnnoiseless
settings-slots-no-backends = No slot-extraction backends installed. Download the GLiNER model files into resources/models/gliner_small-v2.1 (or gliner_multi-v2.1) - the descriptors are already there and the backend appears here once the weights are in place.