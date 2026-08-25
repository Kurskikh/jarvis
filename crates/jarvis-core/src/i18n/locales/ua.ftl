# ### APP INFO
app-name = JARVIS
app-description = Голосовий асистент

# ### TRAY MENU
tray-stop-speaking = Замовкнути
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
settings-wake-word-desc = Виберіть нейромережу, яка слухає ім’я. Зміна діє одразу, з наступної вашої фрази.
settings-wake-score = Суворість активації
settings-wake-score-desc = Наскільки точно має збігтися слово-активатор. Добирається за логом, а не на око: там на кожну спробу пишеться набраний бал і поріг, який треба було взяти. Нижче — реагує охочіше, але й на чужу мову; вище — доводиться кликати двічі, і початок команди губиться. Від 30 до 95, типово 62. Працює для Rustpotter.
settings-stt-engine = Розпізнавання мовлення
settings-intent-engine = Визначення наміру
settings-intent-engine-desc = Виберіть нейромережу для розпізнавання команд.
settings-noise-suppression = Шумозаглушення
settings-noise-suppression-desc = Зменшує фоновий шум. Може негативно впливати на розпізнавання.
settings-vad = Визначення голосу (VAD)
settings-vad-desc = Пропускає тишу, економить ресурси CPU.
settings-stt = Рушій розпізнавання команд
settings-stt-desc = Що розшифровує сказане ПІСЛЯ слова-активатора. Саме слово «Джарвіс» завжди ловить Vosk, і цей вибір його не стосується: там потрібен словник із восьми слів, інакше довелося б цілодобово розшифровувати всі звуки в кімнаті. Зміна діє одразу, з наступної вашої фрази.
settings-vad-threshold = Поріг гучності
settings-vad-threshold-desc = Наскільки гучним має бути звук, щоб вважатися мовленням. Це порівняння гучності, а не розуміння мовлення: нижче — асистент прокидається на вентилятор, вище — не чує тихого голосу. Залежить від мікрофона й кімнати, тому й винесено. Від 10 до 2000, типово 100.
settings-speech-pause = Пауза, що завершує фразу (мс)
settings-speech-pause-desc = Скільки тиші після ваших слів означає, що ви договорили. Потокове розпізнавання не видає кінець фрази, доки не почує тишу: заміряно — без неї «Доброе утро, сэр» перетворюється на «доб». Коротше — обірве на роздумі, довше — відповість із затримкою. Vosk вирішує це сам і налаштування не читає. Від 200 до 3000.
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
settings-api-key = Ключ доступу

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

# ### LLM
settings-llm = Мовна модель
settings-llm-enabled = Відповідь мовної моделі
settings-llm-enabled-desc = Якщо команду не знайдено, запитати локальну мовну модель і показати відповідь тут.
settings-llm-base-url = Адреса
settings-llm-base-url-desc =
    Сумісна з OpenAI адреса. LM Studio: http://127.0.0.1:1234/v1
    Ollama: http://127.0.0.1:11434/v1
settings-llm-model = Модель
settings-llm-model-desc = Обирається з тих, що сервер повідомляє сам. Якщо сервер не відповідає, ім’я можна ввести вручну.
settings-llm-models-refresh = Оновити список
settings-llm-models-loading = Питаю сервер…
settings-llm-models-empty = Сервер відповідає, але не назвав жодної моделі — схоже, жодну не завантажено.
settings-llm-timeout = Тайм-аут (секунди)
settings-llm-timeout-desc = Перше завантаження моделі може тривати хвилину. Від 10 до 600.
settings-llm-max-tokens = Ліміт токенів відповіді
settings-llm-max-tokens-desc = Загальний бюджет на відповідь. Міркувальні моделі витрачають його і на роздуми, тож піднімати бюджет після порожньої відповіді зазвичай неправильно — це дасть думати довше, а не відповісти. Спершу вимкніть роздуми. Від 64 до 32768.
settings-llm-thinking = Міркування моделі
settings-llm-thinking-desc = Міркувальні моделі спершу думають, і це коштує секунд, а іноді всього бюджету — тоді відповідь приходить порожньою. Вимкнення надсилається двома способами одразу: полем у запиті, яке розуміють LM Studio, llama.cpp і vLLM, та директивою в промпті для тих, хто знає лише її.
settings-llm-thinking-auto = Як вирішить модель
settings-llm-thinking-off = Вимкнути
settings-llm-system-prompt = Системний промпт
settings-llm-system-prompt-desc = Надсилається перед кожним питанням. Залиште порожнім, щоб не надсилати.
settings-llm-allow-remote = Дозволити віддалену адресу
settings-llm-allow-remote-desc = Вимкнено — приймаються лише локальні адреси. Увімкнення надішле вашу мову на іншу машину.
settings-llm-speak = Озвучувати відповіді
settings-llm-speak-desc = Читати відповіді вголос голосом асистента. Потрібен запущений сайдкар синтезу; без нього відповіді лишаються текстом.
settings-llm-tts-url = Сайдкар синтезу
settings-llm-tts-url-desc = Де слухає сайдкар. Лише локальна адреса: сайдкар — локальний процес, надсилати мовлення назовні немає причин.
settings-llm-tts-url-bad = Лише локальна адреса. Мовлення синтезується на цій машині й назовні не йде.
settings-llm-tts-check = Перевірити зв’язок
settings-llm-tts-checking = Перевіряю…
settings-llm-tts-ok = Сайдкар відповідає
settings-llm-tts-hz = Гц
settings-llm-tts-mode = Режим синтезу
settings-llm-tts-mode-desc = Потоковий починає говорити приблизно на півтори секунди раніше і значно стабільніший від відповіді до відповіді. Цілком чекає всю відповідь; лишено для порівняння.
settings-llm-tts-mode-stream = Потоковий
settings-llm-tts-mode-sentence = Цілком
settings-llm-tts-python = Інтерпретатор сайдкара
settings-llm-tts-python-desc = Python з оточення, де встановлено рушій синтезу. Заповнюйте, лише якщо хочете, щоб Джарвіс піднімав сайдкар сам.
settings-llm-tts-script = Скрипт сайдкара
settings-llm-tts-script-desc = Повний шлях до скрипта сайдкара. Потрібен, лише якщо заповнено інтерпретатор вище.
settings-llm-tts-advanced = Додатково: хай Джарвіс сам запускає сайдкар
settings-llm-tts-advanced-desc = Лишіть обидва поля порожніми, якщо запускаєте сайдкар самі — Джарвіс просто підключиться за адресою вище. Заповніть обидва, і він піднімати­ме його на старті та гаситиме на виході.
settings-llm-tts-half = Заповнено лише одне поле з двох. Щоб Джарвіс запускав сайдкар сам, потрібні обидва; інакше очистіть обидва й запускайте сайдкар самі.
settings-llm-tts-instruct = Інструкція голосу
settings-llm-tts-instruct-desc = Як говорити, а не що. Порожнє поле — клонування за зразком, і це рекомендований варіант: інструкція скасовує манеру мовлення з вашого зразка й лишає тільки тембр. Заміряно: китайська інструкція працює, англійська кривить слова, російську модель зачитує вголос замість відповіді.
settings-follow-up = Слухати після відповіді
settings-follow-up-desc = Скільки секунд мікрофон лишається відкритим після того, як асистент договорив, щоб наступне питання можна було поставити без «Джарвіс». Відлік іде з кінця мовлення, а не з моменту питання. 0 — вимкнути.
settings-dialogue-exit = Пауза до виходу з діалогу
settings-dialogue-exit-desc = Скільки секунд мовчання завершують розмову, розпочату словами «давай поговоримо». Відлік іде з миті, коли асистент договорив. Вийти можна й уголос: «стоп», «досить», «до побачення». Усередині діалогу команди не виконуються — усе сказане йде до мовної моделі, а заготовки не програються.
settings-duck = Приглушувати решту
settings-duck-desc = Поки Джарвіс слухає й відповідає, музика та інші звуки стають тихішими, а потім повертаються. Приглушується лише те, що саме зараз звучить, і лише через мікшер Windows — загальний регулятор не чіпається. Якщо ви тим часом самі посунете повзунок застосунку, він лишиться там, де ви його поставили.
settings-duck-level = Наскільки тихіше
settings-duck-level-desc = Скільки відсотків попередньої гучності лишається в решти. Менше — тихіше: 20 приблизно як робить сама Windows під час дзвінка, 0 — повна тиша. Більше майже нічого не змінює: на 80 різниця лише два децибели, її не чути. Якщо Джарвіса перекриває музика, почніть з 10-20. Від 0 до 90.
settings-voice-volume = Гучність голосу
settings-voice-volume-desc = Наскільки гучно звучить сам Джарвіс. 100 — як записано. Записи зроблені майже під стелю, тож вище 150 найгучніші фрази можуть захрипіти — запасу там небагато. Якщо Джарвіса заглушає музика, спершу зменшіть «Наскільки тихіше»: там запас значно більший. Від 50 до 200.
settings-llm-history = Пам’ятати розмову
settings-llm-history-desc = Асистент триматиме в пам’яті попередні запитання та власні відповіді, тож «а завтра?» зрозуміється правильно. Кожен обмін їде до моделі разом із наступним запитанням, тому довга нитка відповідає повільніше.
settings-llm-history-turns = Глибина пам’яті
settings-llm-history-turns-desc = Скільки останніх пар «запитання — відповідь» їде разом із новим запитанням. Від 1 до 20.
settings-llm-history-idle = Забувати після мовчання
settings-llm-history-idle-desc = Через скільки хвилин тиші розмова вважається завершеною. Голосом не натиснути «новий діалог», тож нитка обривається сама; сказати «стоп» або «забудь» обриває її одразу. Від 1 до 240.
settings-llm-remote-blocked = Це не локальна адреса. Поки «Дозволити віддалену адресу» вимкнено, туди нічого не надсилається.
settings-api-key-desc = Токен для адреси вище. LM Studio — вкладка Developer. Ollama токен не потрібен.
settings-saved-restart-hint = Асистент недоступний, тому він може продовжувати працювати з попередніми налаштуваннями. Перезапустіть його, щоб застосувати їх.

# llm answer panel
llm-thinking = Думаю...
llm-answer = Відповідь
llm-stop-speaking = Замовкнути
llm-error-connect = Не вдалося зв'язатися з моделлю
llm-error-unauthorized = Адреса відхилила токен
llm-error-model-not-found = Модель недоступна
llm-error-timeout = Відповідь не надійшла вчасно
llm-error-truncated = Модель не встигла відповісти в межах ліміту
llm-error-malformed = Неочікувана відповідь від сервера
llm-error-http-status = Сервер повернув помилку
llm-error-transport = Запит не вдався
llm-error-not-configured = Мовну модель не налаштовано

# BACKEND OPTION LABELS
backend-none = Вимкнено
backend-intent-classifier = Intent Classifier
backend-energy = За рівнем гучності
backend-nnnoiseless = Nnnoiseless
backend-silero-vad = Silero VAD
settings-slots-no-backends = Бекенди вилучення слотів не встановлено. Завантажте файли моделі GLiNER до resources/models/gliner_small-v2.1 (або gliner_multi-v2.1) - описи моделей уже на місці, бекенд зʼявиться тут одразу після завантаження ваг.