# Передача работы: EffectCraft — библиотека плагинов сообщества (+ отложенный OFX-зонд для Sapphire)

Документ для ИИ-агента, который продолжит работу на другом устройстве. Прочитай целиком, потом
`CLAUDE.md` и `AGENTS.md` в корне репозитория: их правила обязательны (особенно «Never crash» и
quality gates).

Пользователь общается по-русски. Отвечай по-русски, код и коммиты пиши по-английски в стиле репозитория.

---

## 0. Как восстановить рабочее состояние

**Вариант А (основной): WIP-ветка в форке пользователя.**
```bash
git clone https://github.com/storytold/effectcraft.git && cd effectcraft
git remote add fork git@github.com:iseemhlyfta/effectcraft.git
git fetch fork plugin-library && git checkout -b plugin-library fork/plugin-library
```
В ветке один WIP-коммит поверх `01c904b0`, с пометкой «not CI-green». Пользователь разрешил его как
исключение из правила «только зелёные коммиты». Этот файл лежит в ветке как
`HANDOFF-plugin-library.md`. **Перед финальным коммитом удали его** и замени WIP-коммит чистым
(`git reset --soft 01c904b0` или новый коммит поверх, по желанию пользователя).

**Вариант Б: патч.** Если ветки в форке нет, вся работа лежит в `plugin-library.patch` (передаётся вместе с этим
файлом).

```bash
git clone https://github.com/storytold/effectcraft.git   # или существующий клон
cd effectcraft
git checkout -b plugin-library 01c904b0   # базовый коммит (main на 2026-10-27)
git apply --whitespace=nowarn /путь/к/plugin-library.patch
git status   # 13 изменённых + 2 новых файла
```

Если `01c904b0` уже далеко позади `main`, накати на свежий `main` (`git apply -3`) и разреши конфликты.
Скорее всего они будут только в `crates/engine/src/lib.rs` (поля `Session`) и `crates/engine/src/cmd.rs`
(список `specs()`).

Тулчейн: Rust stable ≥ 1.95 (проверено на 1.99.1), компоненты `clippy` и `rustfmt`, таргет
`wasm32-unknown-unknown` (нужен для `cargo xtask ci` и для сборки примера плагина в тестах:
`rustup target add wasm32-unknown-unknown`). Первая сборка долгая, 10+ минут.

---

## 1. Контекст и решения пользователя

**Исходный вопрос:** можно ли подключать в EffectCraft сторонние плагины вроде Boris FX Sapphire.

Результат анализа (уже сообщён пользователю):
- Сейчас есть только WASM-плагины (API v1, песочница wasmi: без импортов, с лимитами fuel и памяти; `crates/plugin`,
  `crates/effects/src/plugin.rs`, `docs/plugins.md`) и Rust-плагины через типаж `EffectPlugin`.
- Нативные плагины (`.aex`, `.ofx`) загружать нельзя: `unsafe_code = "forbid"` на весь workspace
  (`Cargo.toml:135`), в `docs/gaps.md` G8 / строка 19 написано «SDK hosting needs an owner decision»
  (issues #600, #327 OpenFX, #376 plug-in manager).
- Хостить `.aex` (AE SDK) не рекомендовано: огромный API, лицензия Adobe SDK, clean-room правила проекта.
  Декомпилировать или переписывать чужие `.aex` нельзя (EULA, clean-room). Можно только писать свои аналоги
  по наблюдаемому поведению.
- Реалистичный путь для Sapphire: **OpenFX-хост** в отдельном процессе-мосте (Sapphire OFX существует,
  заголовки OFX под BSD-3). См. раздел 5. Отложено.

**Новая идея пользователя:** библиотека плагинов сообщества. Люди публикуют плагины, пользователь ставит их
из отдельного блока в приложении, а в самом приложении остаётся меньше встроенного. На уточняющие вопросы
пользователь ответил:
1. Ставить **только WASM-эффекты** (не скрипты и не пресеты): они в песочнице, поэтому чужой код безопасен.
2. **Только общий каталог**: один `index.json` в Git-репозитории, пополняется через PR с ревью. Установки по
   произвольной ссылке нет.
3. **Сначала библиотека**, OFX-зонд потом.

---

## 2. Архитектура (что реализовано)

### Модель распространения
Каталог — `index.json` в репозитории (константа `CATALOG_URL` =
`https://raw.githubusercontent.com/storytold/effectcraft-plugins/main/index.json`). **Такого репозитория
пока нет**, его должен создать владелец проекта. Пусть пользователь решит, где он будет. Формат:

```json
{"schema": 1, "packages": [{
  "id": "org.example.glow-pack", "name": "Glow Pack", "author": "…", "license": "MIT",
  "repo": "https://github.com/example/glow-pack", "description": "…", "tags": ["stylize"],
  "versions": [{"version": "1.2.0", "api": 1, "minApp": "0.17.0",
    "url": "https://github.com/example/glow-pack/releases/download/v1.2.0/glow.wasm",
    "sha256": "<64 hex>", "size": 12345, "effects": ["org.example.glow"]}]}]}
```

Приложение ничего не компилирует. Оно качает готовый `.wasm` (GitHub Release asset), сверяет SHA-256 из
каталога и грузит его обычным загрузчиком плагинов (та же песочница).

### Важное отступление от плана: сеть через системный `curl`
По первоначальному плану был хук `NetFetch` на `ureq` в `crates/host`. Но в коде нашёлся прецедент:
`crates/engine/src/analysis/models.rs` (модели Roto Brush). Он **намеренно не линкует сетевой код** и
качает через системный `curl` с проверкой SHA-256. Библиотека сделана так же: без новых зависимостей и без
изменений слоёв. Фоновая задача, `Status{busy,error,done}`, тост через `poll_*` и параметр `wait: true`
для агентов и тестов повторяют `models.rs`.

### Файлы (патч)
| Файл | Что сделано |
|---|---|
| `crates/engine/src/system/library.rs` (**новый**, ~700 строк) | Весь модуль: типы `Catalog/Package/Release/Installed`, `parse_catalog` (кривые записи отбрасываются с предупреждением, схема ≠ 1 → ошибка «update EffectCraft», лимит 4 MiB), `latest_compatible` (API + `minApp`), сравнение версий, `valid_id`, правила URL (`https://` только на `github.com`, `raw.githubusercontent.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com`; `file://` только когда сам каталог `file://`, для локальной проверки авторами), `fetch` (curl `--proto =https --proto-redir =https --max-filesize`), установка в `<Plug-ins>/Library/<id>/<version>/<id>.wasm` + `installed.json`, кеш каталога `catalog.json` (работа офлайн), `load_installed` (проверка SHA-256 при старте), команды, юнит-тесты |
| `crates/engine/src/system.rs` | `pub mod library;` |
| `crates/engine/src/lib.rs` | `pub use system::library;`; поля `Session::plugins_dir: Option<PathBuf>` и `Session::library: library::Library` (+ в `Default`) |
| `crates/engine/src/cmd.rs` | `v.extend(crate::library::specs());` |
| `crates/engine/src/analysis/roto.rs` | `poll_roto` вызывает `self.poll_library()` рядом с `poll_models()` (тосты по завершении фоновой задачи) |
| `crates/engine/src/settings/prefs/dialog.rs` | `Item::PluginLibrary`; страница настроек `id: "library"`, «Plug-in Library»; строка в `pages_json` |
| `crates/engine/src/settings/prefs/document.rs` | `page_id`: `library`/`pluginlibrary`/`plugins` → `library` |
| `crates/engine/src/menus.rs` | Effect ▸ `Plug-in Library... \| prefs.open {"page":"library"}` |
| `crates/host/src/lib.rs` | `plugins_dir: config_dir().map(\|d\| d.join("Plug-ins"))` в `session()` |
| `apps/effectcraft/src/main.rs` | При старте, после `effect.plugins.load {folder}`, выполняется `library.loadInstalled` (загрузчик плоской папки не рекурсивный, двойной загрузки нет) |
| `crates/ui-egui/src/panels/settings.rs` | `library_ui`: поиск, Fetch Catalog / Refresh, Update All, карточки пакетов (имя, версия, автор, лицензия, размер, ссылка Source, список эффектов, статусы «Installed / update available / Restart EffectCraft to finish / Needs a newer EffectCraft»), кнопки Install / Update / Remove, спиннер, ошибка, папка. Automation ids: `settings.library.query`, `.refresh`, `.updateAll`, `.install.<id>`, `.update.<id>`, `.remove.<id>` |
| `crates/ui-egui/src/panels/dialogs.rs` | `DialogState::library_query: String` |
| `crates/host/tests/it/library.rs` (**новый**) | 4 интеграционных теста против локального `file://`-каталога (см. ниже) |
| `crates/host/tests/it/main.rs`, `plugins.rs` | `mod library;`; `wat_plugin` стал `pub(super)` для переиспользования |

### Команды (`library.*`; всё вызывается из UI, CLI, control channel и MCP)
- `library.list {query?, tag?, installed?}` (query) → `{catalog, fetched, packages:[{id,name,author,description,license,repo,tags,latest,size,effects,installed,updateAvailable,needsNewerApp,loaded,restartNeeded,unlisted?}], tags, busy, error, folder, available}`
- `library.refresh {catalog?, wait?}`: скачать каталог (по умолчанию `CATALOG_URL`)
- `library.install {id, version?, wait?}`
- `library.update {id?, wait?}`: без `wait` ставит только первый пакет из очереди (воркер берёт одну задачу)
- `library.uninstall {id}`
- `library.missing {}` (query): эффекты проекта, которых нет в сессии, и пакеты каталога, которые их дают
- `library.loadInstalled {}`: загрузка установленных при старте

### Ограничение, о котором надо помнить
Реестр эффектов только дополняется (`OnceLock`-слоты, `crates/effects/src/plugin.rs:236`, `MAX_PLUGINS = 64`).
Поэтому удаление или обновление **загруженного** пакета вступает в силу после перезапуска (`restartNeeded`,
`Status::restart`). Новую версию при обновлении загруженного пакета до перезапуска проверить нельзя: при
старте её проверит `loadInstalled`.

---

## 3. Статус проверки

- ✅ `cargo test -p effectcraft-engine --lib library`: **6/6 юнит-тестов прошли** (версии, id, выбор релиза,
  разбор каталога, правила URL, пути модулей).
- ✅ `effectcraft-engine` со всеми правками компилируется (юнит-тесты собрались). Изменения в `effectcraft-host`
  и `apps/effectcraft` (по одной строке) после правок не собирались.
- ❌ **Не запускались** интеграционные тесты `crates/host/tests/it/library.rs`.
- ❌ **Не компилировался** `effectcraft-ui-egui` с новым `library_ui`.
- ❌ Не запускались `cargo fmt`, `clippy -D warnings`, `cargo xtask ci` (layers, assets, wasm).
- ❌ UI ни разу не смотрели глазами (headless-снимок).

---

## 4. Что делать дальше (по порядку)

1. **Собрать и прогнать:**
   ```bash
   cargo test -p effectcraft-host --test it library
   cargo check -p effectcraft-ui-egui
   cargo test -p effectcraft-engine        # могут сработать тесты на меню, команды, страницы настроек
   ```
   Где ждать проблем:
   - `file://`-URL на Windows: тест строит `file:///C:/...`, разбор в `file_url_path` (`library.rs`).
   - Тесты-«стражи» реестра команд, меню и настроек: `crates/engine/src/menus/tests.rs`,
     `crates/engine/src/tests/session.rs`, `crates/automation/src/tests.rs`, `crates/ui-egui/tests/it/ui_agent_audit.rs`.
     Им могут понадобиться документация команд, i18n-строки или automation ids.
   - **i18n:** новые строки UI (`"Fetch Catalog"`, `"Restart EffectCraft to finish"`, `"Installed {}"` и др.)
     и метки команд (`"Plug-in Library"`, `"Refresh Plug-in Catalog"`, …). В `crates/ui-egui/src/i18n.rs` есть
     каталоги zh-hans / zh-hant / ja (например `("file.findMissing", "Find Missing Effects", "…")`). Посмотри,
     есть ли тест полноты переводов, и добавь строки.
   - `docs/preferences.md` может проверяться тестом на соответствие `prefs::pages()`: добавь туда страницу
     Plug-in Library.
2. **`MAX_PLUGINS` 64 → 256** (`crates/effects/src/plugin.rs:37`) с понятной ошибкой при переполнении.
   Это пункт плана, он ещё не сделан.
3. **Документация:**
   - `docs/plugins.md`: раздел `## Plug-in Library` (на него ссылается doc-комментарий модуля:
     `docs/plugins.md#plug-in-library`). Для пользователей: где находится, как работает, перезапуск, папка.
     Для авторов: собрать `.wasm`, выложить GitHub Release, посчитать sha256, сделать PR с записью в `index.json`,
     проверить локально через `library.refresh {"catalog": "file:///…/index.json"}`.
   - `docs/gaps.md`: G8 и строка 19, прогресс по #376 (запись в changelog внизу файла по его формату).
   - `examples/plugins/posterize-bands/`: пример записи каталога (например `catalog-entry.json` или раздел в README).
4. **Посмотреть UI** (правило CLAUDE.md «look at the result»):
   ```bash
   cargo run -p effectcraft-ui-egui --example snapshot -- --out lib.png \
     --step '{"method":"engine.execute","params":{"command":"prefs.open","params":{"page":"library"}}}'
   ```
   Для снимка с данными сначала выполни шаг `library.refresh` с `{"catalog":"file:///…","wait":true}` на
   локальный каталог.
5. **`cargo xtask ci`** до зелёного состояния, затем **один коммит** `G8: plug-in library` в ветке
   `plugin-library` с атрибуцией по правилам репозитория. Пушить и открывать PR **только по просьбе пользователя**.
   `origin` — это `git@github.com:storytold/effectcraft.git` (апстрим); возможно, пользователь захочет свой форк.
6. Сказать пользователю, что репозиторий-каталог `storytold/effectcraft-plugins` нужно создать (с CI,
   который проверяет схему и sha256 и прогоняет модуль через загрузчик), иначе вкладка будет пустой.

Возможные улучшения (не обязательны, сначала спросить): настройка «URL каталога» в Prefs (сейчас только
параметр команды), кнопка «Найти в библиотеке» в File ▸ Dependencies ▸ Find Missing Effects (`library.missing`
уже есть), фоновое обновление всех пакетов разом.

---

## 5. Отложенная задача: OFX-зонд для Sapphire

Пользователь ставит Boris FX Sapphire и хочет проверить, удастся ли подгрузить его OFX-версию.
- Ветка `ofx-probe` и worktree `D:\github\effectcraft-ofx` существуют **только на старом устройстве** и
  **пустые**. На новом устройстве создай ветку заново от `main`.
- Цель первого шага — дешёвый эксперимент, который снимает главный риск: согласится ли Sapphire работать с
  неизвестным хостом (лицензирование). Рендер пока не нужен.
- Устройство: отдельный бинарник **вне workspace** (например `tools/ofx-probe/Cargo.toml` с собственной пустой
  таблицей `[workspace]`, чтобы не попасть под `unsafe_code = "forbid"` и glob `crates/*`). Зависимость
  `libloading`, FFI написан вручную по заголовкам OpenFX (BSD-3:
  `https://raw.githubusercontent.com/AcademySoftwareFoundation/openfx/main/include/ofx*.h`).
- Путь на Windows: `C:\Program Files\Common Files\OFX\Plugins\Sapphire.ofx.bundle\Contents\Win64\Sapphire.ofx`
  (в общем случае сканировать `…\OFX\Plugins\*.ofx.bundle`).
- Шаги: `OfxGetNumberOfPlugins`, `OfxGetPlugin(i)` → `OfxPlugin{pluginApi, apiVersion, pluginIdentifier,
  versionMajor/Minor, setHost, mainEntry}`; `setHost(&OfxHost{host: propset, fetchSuite})`; `mainEntry` с
  `"OfxActionLoad"`, `"OfxActionDescribe"` (дескриптор с property set),
  `"OfxImageEffectActionDescribeInContext"` (контекст Filter или General). Вывести JSON: id, label, grouping,
  contexts, clips, params.
- Минимальный набор suite: `OfxPropertySuite` v1 (18 функций set/get Pointer/String/Double/Int + N-варианты,
  `propReset`, `propGetDimension`; строки хранить как `CString`, чтобы указатели жили долго),
  `OfxImageEffectSuite` v1 (`getPropertySet`, `getParamSet`, `clipDefine`, остальное → `kOfxStatErrUnsupported`),
  `OfxParameterSuite` v1 (`paramDefine`, `paramSetGetPropertySet`), `OfxMemorySuite`, `OfxMultiThreadSuite`,
  `OfxMessageSuite` (функция variadic: объявить с фиксированными аргументами и печатать только `format`).
  На неизвестные suite `fetchSuite` возвращает NULL. Коды статусов: OK=0, Failed=1, ErrUnknown=3,
  ErrUnsupported=5, ErrBadHandle=9, ErrBadIndex=10, **Unlicensed=15**.
- Свойства хоста: `OfxPropName`, `OfxPropLabel`, `OfxPropAPIVersion`, `OfxImageEffectPropSupportedContexts`,
  `OfxImageEffectPropSupportedPixelDepths` (`OfxBitDepthFloat`), `OfxImageEffectPropSupportedComponents`
  (`OfxImageComponentRGBA`) и т.д. **Не подделывать имя хоста** (Nuke, Resolve…), чтобы обойти лицензирование:
  если Sapphire откажет, честно сообщить пользователю.
- Известная сложность для следующего этапа (рендер): `paramGetValue`/`paramSetValue` в OFX — C-variadic. На
  stable Rust variadic-функции определять нельзя. На x86-64 (Win64, SysV) можно принять фиксированные
  указатели, на Apple arm64 так не получится. Ещё одна особенность: в Win64 double из varargs дублируются в
  целочисленных регистрах.
- Полноценная интеграция потом: процесс-мост, кадры через shared memory, прокси `EffectPlugin` в
  `crates/effects`. Текущий контракт узкий: один вход, нет RoI/RoD, мало типов параметров, `MAX_PLUGINS`.
  Сапфиру нужно около 270 эффектов. Это решение уровня владельца проекта (G8).

---

## 6. Заметки об окружении старого устройства (на случай, если важно)
- Windows 11, Rust поставлен через rustup с хостом `x86_64-pc-windows-gnu` (MSVC Build Tools не было).
- Однажды rustup падал с DNS-ошибкой на `static.rust-lang.org`, повторная попытка прошла.
- Сервера MCP `effectcraft` / `effectcraft-app` не подключались. Для проверки UI используй headless snapshot.
