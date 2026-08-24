# ### APP INFO
app-name = JARVIS
app-description = Voice Assistant

# ### TRAY MENU
tray-stop-speaking = Stop speaking
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
settings-stt = Command recogniser
settings-stt-desc = What transcribes whatever is said AFTER the wake word. The name itself is always caught by Vosk and this choice does not touch it: that needs a grammar of eight words, or the assistant would be decoding every sound in the room all day. A change takes effect on the next start.
settings-vad-threshold = Loudness threshold
settings-vad-threshold-desc = How loud a sound has to be before it counts as speech. This compares loudness, it does not understand speech: lower and the assistant wakes on a fan, higher and a quiet voice goes unheard. It depends on the microphone and the room, which is why it is a setting. From 10 to 2000, 100 by default.
settings-speech-pause = Pause that ends a phrase (ms)
settings-speech-pause-desc = How much silence after your words means you have finished. Streaming recognition will not commit the end of a phrase until it hears silence - measured, without it "Доброе утро, сэр" comes back as "доб". Shorter cuts you off mid-thought, longer delays the answer. Vosk decides this for itself and ignores the setting. From 200 to 3000.
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
settings-api-key = Access key

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

# ### LLM
settings-llm = LLM
settings-llm-enabled = LLM fallback
settings-llm-enabled-desc = When no command matches, ask a local language model and show the answer here.
settings-llm-base-url = Endpoint
settings-llm-base-url-desc =
    OpenAI-compatible address. LM Studio: http://127.0.0.1:1234/v1
    Ollama: http://127.0.0.1:11434/v1
settings-llm-model = Model
settings-llm-model-desc = Picked from what the server reports it can serve. If the server does not answer, the name can still be typed in.
settings-llm-models-refresh = Refresh the list
settings-llm-models-loading = Asking the server…
settings-llm-models-empty = The server answers but named no models - it looks like none are loaded.
settings-llm-timeout = Timeout (seconds)
settings-llm-timeout-desc = A model loading for the first time can take a minute. From 10 to 600.
settings-llm-max-tokens = Answer token limit
settings-llm-max-tokens-desc = The whole budget for one answer. A reasoning model spends it on thinking too, so raising it after an empty answer usually makes things worse - it buys more thinking, not an answer. Turn thinking off first. From 64 to 32768.
settings-llm-thinking = Model reasoning
settings-llm-thinking-desc = A reasoning model thinks first, which costs seconds and sometimes the whole budget - then the answer comes back empty. Turning it off is sent two ways at once: a request field that LM Studio, llama.cpp and vLLM understand, and a prompt directive for models that only know that one.
settings-llm-thinking-auto = Leave to the model
settings-llm-thinking-off = Off
settings-llm-system-prompt = System prompt
settings-llm-system-prompt-desc = Sent before every question. Leave empty to send none.
settings-llm-allow-remote = Allow a remote endpoint
settings-llm-allow-remote-desc = Off, only loopback addresses are accepted. Turning this on sends your speech to another machine.
settings-llm-speak = Speak the answers
settings-llm-speak-desc = Read answers out loud in the assistant's voice. Needs the speech sidecar running; without it answers stay written.
settings-llm-tts-url = Speech sidecar
settings-llm-tts-url-desc = Where the sidecar listens. Loopback only - the sidecar is a local process and there is no reason to send speech elsewhere.
settings-llm-tts-url-bad = Loopback addresses only. Speech is synthesised on this machine and does not leave it.
settings-llm-tts-check = Check the connection
settings-llm-tts-checking = Checking…
settings-llm-tts-ok = The sidecar answers
settings-llm-tts-hz = Hz
settings-llm-tts-mode = Synthesis mode
settings-llm-tts-mode-desc = Streaming starts speaking about a second and a half sooner and varies far less between answers. One shot waits for the whole answer; kept for comparison.
settings-llm-tts-mode-stream = Streaming
settings-llm-tts-mode-sentence = One shot
settings-llm-tts-python = Sidecar interpreter
settings-llm-tts-python-desc = Python from the environment the synthesis engine is installed in. Fill this in only if you want jarvis to start the sidecar itself.
settings-llm-tts-script = Sidecar script
settings-llm-tts-script-desc = Full path to the sidecar script. Needed only if the interpreter above is filled in.
settings-llm-tts-advanced = Advanced: let jarvis start the sidecar itself
settings-llm-tts-advanced-desc = Leave both empty if you start the sidecar yourself - jarvis will simply connect to the address above. Fill both in and it will start the sidecar on launch and stop it on exit.
settings-llm-tts-half = Only one of the two is filled in. Starting the sidecar needs both; otherwise clear both and start it yourself.
settings-llm-tts-instruct = Voice instruction
settings-llm-tts-instruct-desc = How to speak, not what to say. Empty means plain cloning from your sample, and that is the recommended setting: an instruction drops the manner copied from the sample and keeps only the timbre. Measured: a Chinese instruction works, an English one mangles words, a Russian one gets read aloud instead of the answer.
settings-follow-up = Keep listening after an answer
settings-follow-up-desc = Seconds to stay listening once the assistant has finished speaking, so the next question needs no wake word. The countdown starts when it stops talking, not when you asked. 0 turns it off.
settings-duck = Quiet everything else
settings-duck-desc = While jarvis is listening and answering, music and other sounds drop and then come back. Only what is actually playing is touched, and only through the Windows mixer - the master volume is left alone. If you move an application's slider yourself in the meantime, it stays where you put it.
settings-duck-level = How much is left
settings-duck-level-desc = What percentage of the former volume is left. 20 is close to what Windows itself does during a call; 0 is silence. From 0 to 90.
settings-llm-history = Remember the conversation
settings-llm-history-desc = The assistant keeps the previous questions and its own answers in mind, so "and tomorrow?" lands as intended. Every exchange travels to the model with the next question, so a long thread answers more slowly.
settings-llm-history-turns = How much to keep
settings-llm-history-turns-desc = How many recent question-and-answer pairs travel with a new question. From 1 to 20.
settings-llm-history-idle = Forget after silence
settings-llm-history-idle-desc = How many minutes of silence end the conversation. A voice cannot press "new chat", so the thread ends by itself; saying "стоп" or "забудь" ends it at once. From 1 to 240.
settings-llm-remote-blocked = Not a loopback address. Nothing is sent there while "Allow a remote endpoint" is off.
settings-api-key-desc = Bearer token for the address above. LM Studio: Developer tab. Ollama needs none.
settings-saved-restart-hint = The assistant could not be reached, so it may still be using its previous settings. Restart it to apply them.

# llm answer panel
llm-thinking = Thinking...
llm-answer = Answer
llm-stop-speaking = Stop speaking
llm-error-connect = Cannot reach the model
llm-error-unauthorized = The endpoint rejected the token
llm-error-model-not-found = Model not available
llm-error-timeout = No answer in time
llm-error-truncated = The model ran out of tokens before answering
llm-error-malformed = Unexpected answer from the endpoint
llm-error-http-status = The endpoint returned an error
llm-error-transport = Request failed
llm-error-not-configured = LLM is not configured

# BACKEND OPTION LABELS (fall back to the English name from the model registry
# when a key is absent - see frontend translate(..., `backend-${id}`, o.name))
backend-none = Disabled
backend-intent-classifier = Intent Classifier
backend-energy = Energy-based
backend-nnnoiseless = Nnnoiseless
settings-slots-no-backends = No slot-extraction backends installed. Download the GLiNER model files into resources/models/gliner_small-v2.1 (or gliner_multi-v2.1) - the descriptors are already there and the backend appears here once the weights are in place.