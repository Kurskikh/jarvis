# APP INFO
app-name = JARVIS
app-description = Голосовой ассистент

# TRAY MENU
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
footer-author = Автор проекта
footer-telegram = Наш телеграм канал
footer-github = Github репозиторий проекта
footer-support = Поддержать проект на

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
settings-beta-feedback = Сообщайте обо всех найденных багах в
settings-beta-bot = наш телеграм бот
settings-open-logs = Открыть папку с логами

# settings - picovoice
settings-attention = Внимание!
settings-picovoice-warning = Эта нейросеть работает не у всех!
settings-picovoice-waiting = Мы ждем официального патча от разработчиков.
settings-picovoice-key-desc = Введите сюда свой ключ Picovoice. Он выдается бесплатно при регистрации в
settings-picovoice-key = Ключ Picovoice

# settings - vosk
settings-auto-detect = Авто-определение
settings-vosk-model = Модель распознавания речи (Vosk)
settings-vosk-model-desc =
    Выберите модель Vosk для распознавания речи.
    Вы можете скачать модели здесь: https://alphacephei.com/vosk/models
settings-models-not-found = Модели не найдены
settings-models-hint = Поместите модели Vosk в папку resources/vosk

# settings - openai
settings-openai-key = Ключ OpenAI
settings-openai-not-supported = В данный момент ChatGPT не поддерживается. Он будет добавлен в ближайших обновлениях.

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

# BACKEND OPTION LABELS
backend-none = Отключено
backend-intent-classifier = Intent Classifier
backend-energy = По уровню громкости
backend-nnnoiseless = Nnnoiseless
settings-slots-no-backends = Бэкенды извлечения слотов не установлены. Скачайте файлы модели GLiNER в resources/models/gliner_small-v2.1 (или gliner_multi-v2.1) - описания моделей уже на месте, бэкенд появится здесь сразу после загрузки весов.