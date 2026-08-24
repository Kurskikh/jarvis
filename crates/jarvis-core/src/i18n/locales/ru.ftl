# APP INFO
app-name = JARVIS
app-description = Голосовой ассистент

# TRAY MENU
tray-stop-speaking = Замолчать
tray-restart = Перезапустить
tray-settings = Настройки
tray-exit = Выход
tray-tooltip = JARVIS - Голосовой ассистент
tray-language = Язык
tray-voice = Голос
tray-wake-word = Движок wake-word
tray-noise-suppression = Шумоподавление
tray-vad = Детекция голоса (VAD)
tray-gain-normalizer = Нормализация громкости

# HEADER
header-commands = КОМАНДЫ
header-settings = НАСТРОЙКИ

# SEARCH
search-placeholder = Введите команду вручную или произнесите «Джарвис» ...

# MAIN PAGE
assistant-not-running = АССИСТЕНТ НЕ ЗАПУЩЕН
assistant-offline-hint = Настроить его можно не запуская.
btn-start = ЗАПУСТИТЬ
btn-starting = ЗАПУСК...

# STATUS
status-disconnected = Отключен
status-standby = Ожидание
status-listening = Слушаю...
status-processing = Обработка...

# STATS
stats-microphone = МИКРОФОН
stats-neural-networks = НЕЙРОСЕТИ
stats-resources = РЕСУРСЫ
stats-system-default = Системный
stats-not-selected = Не выбран
stats-loading = Загрузка...

# FOOTER

# SETTINGS
settings-title = Настройки
settings-general = Основные
settings-devices = Устройства
settings-neural-networks = Нейросети
settings-audio = Аудио
settings-recognition = Распознавание
settings-about = О программе
settings-language = Язык
settings-microphone = Микрофон
settings-microphone-desc = Его будет слушать ассистент.
settings-mic-default = По умолчанию (Система)
settings-voice = Голос ассистента
settings-voice-desc =
    Не все команды работают со всеми звуковыми пакетами.
    Кликните, чтобы прослушать как звучит голос.
settings-wake-word-engine = Движок активации
settings-wake-word-desc = Выберите нейросеть для распознавания активационной фразы.
settings-stt-engine = Распознавание речи
settings-intent-engine = Определение намерения
settings-intent-engine-desc = Выберите нейросеть для распознавания команд.
settings-noise-suppression = Шумоподавление
settings-noise-suppression-desc = Уменьшает фоновый шум. Может негативно влиять на распознавание.
settings-vad = Определение голоса (VAD)
settings-vad-desc = Пропускает тишину, экономит ресурсы CPU.
settings-stt = Движок распознавания команд
settings-stt-desc = Что расшифровывает сказанное ПОСЛЕ слова-активатора. Само слово «Джарвис» всегда ловит Vosk и этот выбор его не касается: там нужен словарь из восьми слов, иначе пришлось бы круглосуточно расшифровывать все звуки в комнате. Смена вступает в силу при следующем запуске.
settings-vad-threshold = Порог громкости
settings-vad-threshold-desc = Насколько громким должен быть звук, чтобы считаться речью. Это сравнение громкости, а не понимание речи: ниже — ассистент просыпается на вентилятор, выше — не слышит тихого голоса. Зависит от микрофона и комнаты, поэтому и вынесено. От 10 до 2000, по умолчанию 100.
settings-speech-pause = Пауза, заканчивающая фразу (мс)
settings-speech-pause-desc = Сколько тишины после ваших слов означает, что вы договорили. Потоковое распознавание не выдаёт конец фразы, пока не услышит тишину: замерено — без неё «Доброе утро, сэр» превращается в «доб». Короче — оборвёт на раздумье, длиннее — ответит с задержкой. Vosk решает это сам и настройку не читает. От 200 до 3000.
settings-gain-normalizer = Нормализация громкости
settings-gain-normalizer-desc = Автоматически регулирует уровень громкости.
settings-api-keys = API Ключи
settings-save = Сохранить
settings-cancel = Отмена
settings-back = Назад
settings-enabled = Включено
settings-disabled = Отключено

# settings - beta notice
settings-beta-title = БЕТА версия!
settings-beta-desc = Часть функций может работать некорректно.
settings-open-logs = Открыть папку с логами

settings-attention = Внимание!

# settings - vosk
settings-auto-detect = Авто-определение
settings-vosk-model = Модель распознавания речи (Vosk)
settings-vosk-model-desc =
    Выберите модель Vosk для распознавания речи.
    Вы можете скачать модели здесь: https://alphacephei.com/vosk/models
settings-models-not-found = Модели не найдены
settings-models-hint = Поместите модели Vosk в папку resources/vosk

# settings - openai
settings-api-key = Ключ доступа

# COMMANDS PAGE
commands-search = Поиск команд...
commands-loading = Загрузка наборов команд...
commands-packs = Наборы
commands-packs-empty = Наборы команд не найдены.
commands-pack-new = Новый набор
commands-pack-name = Имя набора
commands-pack-name-desc = Только буквы, цифры, "_" и "-". Это имя папки в resources/commands.
commands-pack-empty = В этом наборе пока нет команд.
commands-pack-broken = Этот набор не удалось разобрать. Доступно только редактирование TOML.
commands-pack-unmanaged = Ассистент загружает этот набор, но редактор не может работать с папкой с таким именем. Переименуйте её, используя буквы, цифры, "_" и "-".
commands-pack-delete = Удалить набор
commands-pack-delete-confirm = Введите имя набора для подтверждения. Папка со скриптами будет перемещена в resources/.trash и не удаляется автоматически.
commands-open-folder = Открыть папку
commands-command-new = Новая команда
commands-command-delete = Удалить команду
commands-command-delete-confirm = Убрать эту команду из набора? Изменение применится при сохранении.
commands-delete = Удалить
commands-section-general = Основное
commands-section-exec = Выполнение
commands-section-speech = Фразы и звуки
commands-section-slots = Слоты
commands-field-id = ID команды
commands-field-id-desc = Уникален среди всех наборов. Именно его изучает классификатор намерений.
commands-field-type = Тип
commands-field-description = Описание
commands-field-script = Lua скрипт
commands-field-script-desc = Файл .lua в папке набора. Тела скриптов редактируются вне приложения.
commands-field-sandbox = Песочница
commands-field-timeout = Таймаут, мс
commands-field-timeout-desc = Только для Lua. От 100 до 600000.
commands-field-exe = Исполняемый файл или .ahk скрипт
commands-field-cli = Команда оболочки
commands-field-args = Аргументы
commands-field-args-desc = По одному на строку, передаются как есть. Пустая строка - пустой аргумент.
commands-no-exec-params = У этого типа нет параметров выполнения.
commands-phrases = Фразы
commands-phrases-desc = По одной на строку. Имя слота в фигурных скобках обозначает параметр.
commands-sounds = Звуки
commands-sounds-desc = По одному имени на строку, без расширения. Ищутся в выбранном голосе.
commands-sounds-available = Доступно для этого голоса:
commands-sounds-none = Для этого языка в выбранном голосе нет звуков.
commands-slot-name = Имя слота
commands-slot-entity = Сущность
commands-slot-entity-desc = Произвольное описание, которое GLiNER сопоставляет по смыслу, например "название города".
commands-slot-context = Контекстные слова
commands-slot-context-desc = Через запятую.
commands-slot-add = Добавить слот
commands-slot-error-empty = Слот без имени сохранить нельзя. Задайте имя или удалите строку.
commands-slot-error-duplicate = Два слота имеют одинаковое имя:
commands-raw = Исходный TOML
commands-raw-desc = Заменяет весь файл. Это единственный режим, сохраняющий комментарии и ручное форматирование.
commands-struct-desc = Сохранение перезаписывает command.toml. Комментарии и неизвестные ключи не сохраняются - используйте режим TOML, чтобы их сохранить.
commands-other-buffer-raw = На вкладке TOML есть несохранённые изменения. Сохраните или отмените их: иначе это сохранение их сотрёт.
commands-other-buffer-struct = В обычном редакторе есть несохранённые изменения. Сохраните или отмените их: иначе это сохранение их сотрёт.
commands-saved = Набор команд сохранён
commands-deleted = Набор команд удалён
commands-unsaved = Несохранённые изменения
commands-discard = Отменить изменения?
commands-discard-desc = В этом наборе есть изменения, не записанные на диск. Если выйти сейчас, они будут потеряны.
commands-discard-action = Отменить
commands-validation-title = Набор не сохранён
commands-open-failed = Не удалось открыть набор
commands-create-failed = Набор не создан
commands-delete-failed = Набор не удалён
commands-reload-title = Ассистент не применил изменение
commands-errors-title = Ошибки
commands-warnings-title = Предупреждения
commands-reload-pending = Применяем изменения к работающему ассистенту...
commands-reload-ok = Изменения применены
commands-reload-retrained = Фразы изменились, распознавание намерений переобучено.
commands-reload-skipped = Эти наборы не разбираются и исключены из ассистента:
commands-reload-stale = Команды применены, но распознавание намерений не удалось пересобрать - оно всё ещё работает по старым фразам:
commands-reload-offline = Сохранено на диск. Ассистент не запущен, изменения применятся при следующем запуске.
commands-reload-timeout = Сохранено на диск. Ассистент пока не подтвердил - при большом изменении фраз переобучение занимает время.
commands-reload-failed = Ассистент отклонил перезагрузку
cmdtype-voice = Только голосовой ответ
cmdtype-lua = Lua скрипт
cmdtype-ahk = AutoHotkey
cmdtype-cli = Команда оболочки
cmdtype-terminate = Выход из ассистента
cmdtype-stop_chaining = Остановить цепочку команд
commands-sandbox-minimal = Минимальная
commands-sandbox-standard = Стандартная
commands-sandbox-full = Полная

# ERRORS
error-generic = Произошла ошибка
error-connection = Ошибка подключения
error-not-found = Не найдено

# NOTIFICATIONS
notification-saved = Настройки сохранены!
notification-error = Ошибка
notification-assistant-started = Ассистент запущен
notification-assistant-stopped = Ассистент остановлен

# SLOTS EXTRACTION
settings-slot-engine = Извлечение параметров
settings-slot-engine-desc = Извлекает параметры из голосовых команд (напр. название города, число).
settings-gliner-model = Модель GLiNER ONNX
settings-gliner-model-desc =
    Выберите вариант модели.
    Квантизированные модели (int8, uint8) быстрее, но менее точны.
settings-gliner-models-hint = Модели GLiNER не найдены.

# ETC
search-error-not-running = Ассистент не запущен
search-error-failed = Не удалось выполнить команду
settings-no-voices = Голоса не найдены

# ### LLM
settings-llm = Языковая модель
settings-llm-enabled = Ответ языковой модели
settings-llm-enabled-desc = Если команда не найдена, спросить локальную языковую модель и показать ответ здесь.
settings-llm-base-url = Адрес
settings-llm-base-url-desc =
    Совместимый с OpenAI адрес. LM Studio: http://127.0.0.1:1234/v1
    Ollama: http://127.0.0.1:11434/v1
settings-llm-model = Модель
settings-llm-model-desc = Выбирается из тех, что сервер сообщает сам. Если сервер не отвечает, имя можно ввести вручную.
settings-llm-models-refresh = Обновить список
settings-llm-models-loading = Спрашиваю сервер…
settings-llm-models-empty = Сервер отвечает, но не назвал ни одной модели — похоже, ни одна не загружена.
settings-llm-timeout = Тайм-аут (секунды)
settings-llm-timeout-desc = Первая загрузка модели может занять минуту. От 10 до 600.
settings-llm-max-tokens = Лимит токенов ответа
settings-llm-max-tokens-desc = Общий бюджет на ответ. Рассуждающие модели тратят его и на размышления, и при пустом ответе поднимать бюджет обычно неправильно — он даст думать дольше, а не ответить. Сначала выключите размышления. От 64 до 32768.
settings-llm-thinking = Размышления модели
settings-llm-thinking-desc = Рассуждающие модели сначала думают, и это занимает секунды, а иногда весь бюджет — тогда ответ приходит пустым. Выключение отправляется двумя способами сразу: полем в запросе, которое понимают LM Studio, llama.cpp и vLLM, и директивой в промпте для тех, кто знает только её.
settings-llm-thinking-auto = Как решит модель
settings-llm-thinking-off = Выключить
settings-llm-system-prompt = Системный промпт
settings-llm-system-prompt-desc = Отправляется перед каждым вопросом. Оставьте пустым, чтобы не отправлять.
settings-llm-allow-remote = Разрешить удалённый адрес
settings-llm-allow-remote-desc = Выключено — принимаются только локальные адреса. Включение отправит вашу речь на другую машину.
settings-llm-speak = Озвучивать ответы
settings-llm-speak-desc = Читать ответы вслух голосом ассистента. Нужен запущенный сайдкар синтеза; без него ответы остаются текстом.
settings-llm-tts-url = Сайдкар синтеза
settings-llm-tts-url-desc = Где слушает сайдкар. Только локальный адрес: сайдкар — локальный процесс, отправлять речь наружу незачем.
settings-llm-tts-url-bad = Только локальный адрес. Речь синтезируется на этой машине и наружу не уходит.
settings-llm-tts-check = Проверить связь
settings-llm-tts-checking = Проверяю…
settings-llm-tts-ok = Сайдкар отвечает
settings-llm-tts-hz = Гц
settings-llm-tts-mode = Режим синтеза
settings-llm-tts-mode-desc = Потоковый начинает говорить примерно на полторы секунды раньше и куда стабильнее от ответа к ответу. Целиком ждёт весь ответ; оставлен для сравнения.
settings-llm-tts-mode-stream = Потоковый
settings-llm-tts-mode-sentence = Целиком
settings-llm-tts-python = Интерпретатор сайдкара
settings-llm-tts-python-desc = Python из окружения, где стоит движок синтеза. Заполняйте, только если хотите, чтобы Джарвис поднимал сайдкар сам.
settings-llm-tts-script = Скрипт сайдкара
settings-llm-tts-script-desc = Полный путь к скрипту сайдкара. Нужен, только если заполнен интерпретатор выше.
settings-llm-tts-advanced = Дополнительно: пусть Джарвис сам запускает сайдкар
settings-llm-tts-advanced-desc = Оставьте оба поля пустыми, если запускаете сайдкар сами — Джарвис просто подключится по адресу выше. Заполните оба, и он будет поднимать его при старте и гасить при выходе.
settings-llm-tts-half = Заполнено только одно поле из двух. Чтобы Джарвис запускал сайдкар сам, нужны оба; иначе очистите оба и запускайте сайдкар сами.
settings-llm-tts-instruct = Инструкция голосу
settings-llm-tts-instruct-desc = Как говорить, а не что. Пустое поле — клонирование по образцу, и это рекомендуемый вариант: инструкция отменяет манеру речи из вашего образца и оставляет только тембр. Замерено: китайская инструкция работает, английская коверкает слова, русскую модель зачитывает вслух вместо ответа.
settings-follow-up = Слушать после ответа
settings-follow-up-desc = Сколько секунд микрофон остаётся открытым после того, как ассистент договорил, чтобы следующий вопрос можно было задать без «Джарвис». Отсчёт идёт с конца речи, а не с момента вопроса. 0 — выключить.
settings-duck = Приглушать остальное
settings-duck-desc = Пока Джарвис слушает и отвечает, музыка и прочие звуки становятся тише, а потом возвращаются. Приглушается только то, что в этот момент звучит, и только через микшер Windows — общий регулятор не трогается. Если во время этого вы сами подвинете ползунок приложения, он останется там, где вы его поставили.
settings-duck-level = Насколько тише
settings-duck-level-desc = Сколько процентов прежней громкости остаётся. 20 — примерно как делает сама Windows во время звонка. 0 — полная тишина. От 0 до 90.
settings-llm-history = Помнить разговор
settings-llm-history-desc = Ассистент будет держать в уме предыдущие вопросы и свои ответы, так что «а завтра?» поймётся правильно. Каждый обмен уезжает в модель вместе со следующим вопросом, поэтому длинная нить отвечает медленнее.
settings-llm-history-turns = Глубина памяти
settings-llm-history-turns-desc = Сколько последних пар «вопрос — ответ» уходит вместе с новым вопросом. От 1 до 20.
settings-llm-history-idle = Забывать после молчания
settings-llm-history-idle-desc = Через сколько минут тишины разговор считается законченным. Голосом нельзя нажать «новый диалог», поэтому нить обрывается сама; сказать «стоп» или «забудь» обрывает её сразу. От 1 до 240.
settings-llm-remote-blocked = Это не локальный адрес. Пока «Разрешить удалённый адрес» выключено, туда ничего не отправляется.
settings-api-key-desc = Токен для адреса выше. LM Studio — вкладка Developer. Ollama токен не нужен.
settings-saved-restart-hint = Ассистент недоступен, поэтому он может продолжать работать с прежними настройками. Перезапустите его, чтобы применить их.

# llm answer panel
llm-thinking = Думаю...
llm-answer = Ответ
llm-stop-speaking = Замолчать
llm-error-connect = Не удалось связаться с моделью
llm-error-unauthorized = Адрес отклонил токен
llm-error-model-not-found = Модель недоступна
llm-error-timeout = Ответ не пришёл вовремя
llm-error-truncated = Модель не успела ответить в лимит токенов
llm-error-malformed = Неожиданный ответ от сервера
llm-error-http-status = Сервер вернул ошибку
llm-error-transport = Запрос не удался
llm-error-not-configured = Языковая модель не настроена

# BACKEND OPTION LABELS
backend-none = Отключено
backend-intent-classifier = Intent Classifier
backend-energy = По уровню громкости
backend-nnnoiseless = Nnnoiseless
settings-slots-no-backends = Бэкенды извлечения слотов не установлены. Скачайте файлы модели GLiNER в resources/models/gliner_small-v2.1 (или gliner_multi-v2.1) - описания моделей уже на месте, бэкенд появится здесь сразу после загрузки весов.