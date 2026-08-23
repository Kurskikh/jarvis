# ### APP INFO
app-name = JARVIS
app-description = Голосовий асистент

# ### TRAY MENU
tray-restart = Перезапустити
tray-settings = Налаштування
tray-exit = Вихід
tray-tooltip = JARVIS - Голосовий асистент
tray-language = Мова
tray-voice = Голос
tray-wake-word = Рушій детекції
tray-noise-suppression = Шумозаглушення
tray-vad = Детекцiя голосу (VAD)
tray-gain-normalizer = Нормалізація гучності

# ### HEADER
header-commands = КОМАНДИ
header-settings = НАЛАШТУВАННЯ

# ### SEARCH
search-placeholder = Введіть команду вручну або скажіть «Джарвіс» ...

# ### MAIN PAGE
assistant-not-running = АСИСТЕНТ НЕ ЗАПУЩЕНО
assistant-offline-hint = Налаштувати його можна не запускаючи.
btn-start = ЗАПУСТИТИ
btn-starting = ЗАПУСК...

# ### STATUS
status-disconnected = Відключено
status-standby = Очікування
status-listening = Слухаю...
status-processing = Обробка...

# ### STATS
stats-microphone = МІКРОФОН
stats-neural-networks = НЕЙРОМЕРЕЖІ
stats-resources = РЕСУРСИ
stats-system-default = Системний
stats-not-selected = Не вибрано
stats-loading = Завантаження...

# ### FOOTER
footer-author = Автор проєкту
footer-telegram = Наш телеграм канал
footer-github = Github репозиторій проєкту
footer-support = Підтримати проєкт на

# ### SETTINGS
settings-title = Налаштування
settings-general = Основні
settings-devices = Пристрої
settings-neural-networks = Нейромережі
settings-audio = Аудіо
settings-recognition = Розпізнавання
settings-about = Про програму
settings-language = Мова
settings-microphone = Мікрофон
settings-microphone-desc = Його буде слухати асистент.
settings-mic-default = За замовчуванням (Система)
settings-voice = Голос асистента
settings-voice-desc =
    Не всі команди працюють з усіма звуковими пакетами.
    Натисніть, щоб прослухати як звучить голос.
settings-wake-word-engine = Рушій активації
settings-wake-word-desc = Виберіть нейромережу для розпізнавання активаційної фрази.
settings-stt-engine = Розпізнавання мовлення
settings-intent-engine = Визначення наміру
settings-intent-engine-desc = Виберіть нейромережу для розпізнавання команд.
settings-noise-suppression = Шумозаглушення
settings-noise-suppression-desc = Зменшує фоновий шум. Може негативно впливати на розпізнавання.
settings-vad = Визначення голосу (VAD)
settings-vad-desc = Пропускає тишу, економить ресурси CPU.
settings-gain-normalizer = Нормалізація гучності
settings-gain-normalizer-desc = Автоматично регулює рівень гучності.
settings-api-keys = API Ключі
settings-save = Зберегти
settings-cancel = Скасувати
settings-back = Назад
settings-enabled = Увімкнено
settings-disabled = Вимкнено

# settings - beta notice
settings-beta-title = БЕТА версія!
settings-beta-desc = Частина функцій може працювати некоректно.
settings-beta-feedback = Повідомляйте про всі знайдені баги в
settings-beta-bot = наш телеграм бот
settings-open-logs = Відкрити папку з логами

settings-attention = Увага!

# settings - vosk
settings-auto-detect = Авто-визначення
settings-vosk-model = Модель розпізнавання мовлення (Vosk)
settings-vosk-model-desc =
    Виберіть модель Vosk для розпізнавання мовлення.
    Ви можете завантажити моделі тут: https://alphacephei.com/vosk/models
settings-models-not-found = Моделі не знайдено
settings-models-hint = Помістіть моделі Vosk в папку resources/vosk

# settings - openai
settings-openai-key = Ключ OpenAI
settings-openai-not-supported = Наразі ChatGPT не підтримується. Він буде доданий у наступних оновленнях.

# ### COMMANDS PAGE
commands-search = Пошук команд...
commands-loading = Завантаження наборів команд...
commands-packs = Набори
commands-packs-empty = Набори команд не знайдено.
commands-pack-new = Новий набір
commands-pack-name = Назва набору
commands-pack-name-desc = Лише літери, цифри, "_" та "-". Це назва папки в resources/commands.
commands-pack-empty = У цьому наборі поки немає команд.
commands-pack-broken = Цей набір не вдалося розібрати. Доступне лише редагування TOML.
commands-pack-unmanaged = Асистент завантажує цей набір, але редактор не може працювати з папкою з такою назвою. Перейменуйте її, використовуючи літери, цифри, "_" та "-".
commands-pack-delete = Видалити набір
commands-pack-delete-confirm = Введіть назву набору для підтвердження. Папка зі скриптами буде переміщена до resources/.trash і не видаляється автоматично.
commands-open-folder = Відкрити папку
commands-command-new = Нова команда
commands-command-delete = Видалити команду
commands-command-delete-confirm = Прибрати цю команду з набору? Зміна застосується після збереження.
commands-delete = Видалити
commands-section-general = Основне
commands-section-exec = Виконання
commands-section-speech = Фрази та звуки
commands-section-slots = Слоти
commands-field-id = ID команди
commands-field-id-desc = Унікальний серед усіх наборів. Саме його вивчає класифікатор намірів.
commands-field-type = Тип
commands-field-description = Опис
commands-field-script = Lua скрипт
commands-field-script-desc = Файл .lua у папці набору. Тіла скриптів редагуються поза застосунком.
commands-field-sandbox = Пісочниця
commands-field-timeout = Таймаут, мс
commands-field-timeout-desc = Лише для Lua. Від 100 до 600000.
commands-field-exe = Виконуваний файл або .ahk скрипт
commands-field-cli = Команда оболонки
commands-field-args = Аргументи
commands-field-args-desc = По одному на рядок, передаються як є. Порожній рядок - порожній аргумент.
commands-no-exec-params = Цей тип не має параметрів виконання.
commands-phrases = Фрази
commands-phrases-desc = По одній на рядок. Назва слота у фігурних дужках позначає параметр.
commands-sounds = Звуки
commands-sounds-desc = По одній назві на рядок, без розширення. Шукаються у вибраному голосі.
commands-sounds-available = Доступно для цього голосу:
commands-sounds-none = Для цієї мови у вибраному голосі немає звуків.
commands-slot-name = Назва слота
commands-slot-entity = Сутність
commands-slot-entity-desc = Довільний опис, який GLiNER зіставляє за змістом, наприклад "назва міста".
commands-slot-context = Контекстні слова
commands-slot-context-desc = Через кому.
commands-slot-add = Додати слот
commands-slot-error-empty = Слот без назви зберегти не можна. Задайте назву або видаліть рядок.
commands-slot-error-duplicate = Два слоти мають однакову назву:
commands-raw = Вихідний TOML
commands-raw-desc = Замінює весь файл. Це єдиний режим, що зберігає коментарі та ручне форматування.
commands-struct-desc = Збереження перезаписує command.toml. Коментарі та невідомі ключі не зберігаються - використовуйте режим TOML, щоб їх зберегти.
commands-other-buffer-raw = На вкладці TOML є незбережені зміни. Збережіть або скасуйте їх: інакше це збереження їх зітре.
commands-other-buffer-struct = У звичайному редакторі є незбережені зміни. Збережіть або скасуйте їх: інакше це збереження їх зітре.
commands-saved = Набір команд збережено
commands-deleted = Набір команд видалено
commands-unsaved = Незбережені зміни
commands-discard = Скасувати зміни?
commands-discard-desc = У цьому наборі є зміни, не записані на диск. Якщо вийти зараз, їх буде втрачено.
commands-discard-action = Скасувати
commands-validation-title = Набір не збережено
commands-open-failed = Не вдалося відкрити набір
commands-create-failed = Набір не створено
commands-delete-failed = Набір не видалено
commands-reload-title = Асистент не застосував зміну
commands-errors-title = Помилки
commands-warnings-title = Попередження
commands-reload-pending = Застосовуємо зміни до працюючого асистента...
commands-reload-ok = Зміни застосовано
commands-reload-retrained = Фрази змінилися, розпізнавання намірів перенавчено.
commands-reload-skipped = Ці набори не розбираються і виключені з асистента:
commands-reload-stale = Команди застосовано, але розпізнавання намірів не вдалося перезібрати - воно все ще працює за старими фразами:
commands-reload-offline = Збережено на диск. Асистент не запущено, зміни застосуються при наступному запуску.
commands-reload-timeout = Збережено на диск. Асистент поки не підтвердив - при великій зміні фраз перенавчання займає час.
commands-reload-failed = Асистент відхилив перезавантаження
cmdtype-voice = Лише голосова відповідь
cmdtype-lua = Lua скрипт
cmdtype-ahk = AutoHotkey
cmdtype-cli = Команда оболонки
cmdtype-terminate = Вихід з асистента
cmdtype-stop_chaining = Зупинити ланцюжок команд
commands-sandbox-minimal = Мінімальна
commands-sandbox-standard = Стандартна
commands-sandbox-full = Повна

# ### ERRORS
error-generic = Сталася помилка
error-connection = Помилка підключення
error-not-found = Не знайдено

# ### NOTIFICATIONS
notification-saved = Налаштування збережено!
notification-error = Помилка
notification-assistant-started = Асистент запущено
notification-assistant-stopped = Асистент зупинено

# SLOTS EXTRACTION
settings-slot-engine = Витяг параметрів
settings-slot-engine-desc = Витягує параметри з голосових команд (напр. назва міста, число).
settings-gliner-model = Модель GLiNER ONNX
settings-gliner-model-desc = 
    Оберіть варіант моделі.
    Квантизовані моделі (int8, uint8) швидші, але менш точні.
settings-gliner-models-hint = Моделі GLiNER не знайдено.

# ETC
search-error-not-running = Асистент не запущено
search-error-failed = Не вдалося виконати команду
settings-no-voices = Голоси не знайдено

# BACKEND OPTION LABELS
backend-none = Вимкнено
backend-intent-classifier = Intent Classifier
backend-energy = За рівнем гучності
backend-nnnoiseless = Nnnoiseless
settings-slots-no-backends = Бекенди вилучення слотів не встановлено. Завантажте файли моделі GLiNER до resources/models/gliner_small-v2.1 (або gliner_multi-v2.1) - описи моделей уже на місці, бекенд зʼявиться тут одразу після завантаження ваг.