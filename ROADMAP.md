# Hawk Terminal (`hawk-terminal`) — Product Roadmap & Task Board

Централізована система обліку завдань, критичних багів та запланованих модулів.
Синхронізовано з GitHub Projects / Issues: **[Breeze141all/hawk-terminal](https://github.com/Breeze141all/hawk-terminal)**

---

## 1. Завершено (Done / Implemented & Verified)
*Повністю реалізовано в кодовій базі, проходить 155/155 модульних тестів та Clippy.*

- [x] **[#1] 2D Liquidation Heatmap**: Сітка Time × Price, квантування бінів ($100 для BTC), динамічні рівні плечей 10x–100x.
- [x] **[#2] Multi-Period Rolling VWAP**: Ковзні вікна 7d, 30d, 90d, 365d та сесійні періоди зі стандартними відхиленнями 1, 2, 3 SD.
- [x] **[#3] Базова система малювання**: Trendline, Ray, Horizontal Line, Rectangle, Fibonacci Retracement, Text Note.
- [x] **[#4] Market Replay Engine**: Покрокове тікове відтворення, інтерактивний календар вибору дати/часу, Random Bar.
- [x] **[#5] Price Alerts**: Рівні CrossAbove/CrossBelow, аудіо-сигнали, спливаючі Toasts, бейджі на осі Y, менеджер алертів.
- [x] **[#6] Trade Journal (v1 Core)**: Облік торгів, PnL, 3 режими відображення (Disabled, Sidebar 330px, Fullscreen Dashboard).
- [x] **[#7] TPO Multi-Period & Session Merge**: Daily/Weekly/Monthly агрегація, ручне злиття/розділення сесій, літери понад 52.
- [x] **[#8] Footprint Binary Cache & RAM Capping**: LZ4 сирі угоди, `.fp.bin` бінарний кеш кластерів, захист пам'яті.
- [x] **[#9] Order Flow Indicators**: CVD (Cumulative Volume Delta), Bid/Ask Ratio, Position Flow, Net Open Interest.
- [x] **[#10] Workspace Bundle**: Експорт/імпорт налаштувань, лейаутів із захистом від DoS та санітизацією чисел.
- [x] **[#11] Очищення та стабілізація Git**: Єдиний чистий Initial Commit, повна відповідність Rust 1.98+ Clippy, відокремлення від upstream.
- [x] **[#12] Веб-сайт Hawk Terminal**: Лендинг на GitHub Pages у стилі Boon Global (монохром, анімація входу, WebGL-сузір'я).
- [x] **[#13] Обробка аудіо-порогу за обсягом у штуках (Qty Threshold)**: Повний розрахунок сумарного обсягу покупок/продажів та відтворення аудіо.
- [x] **[#14] Безпечна обробка Tick Basis у Comparison Chart**: Fallback на 1m таймфрейм та ігнорування непідтримуваної зміни без аварійного крашу.
- [x] **[#15] Безпечна обробка Tick Basis у Orderbook Heatmap**: Безпечний fallback для TimeSeries та HistoricalDepth на 1m.
- [x] **[#16] Обробка Tick Basis в операціях діапазону KlineChart**: Розрахунок діапазону свічок через `interval_range` на тіковому базисі.
- [x] **[#17] Фіксація кута 45° при затиснутому Shift (45° Angle Snap)**: Прив'язка кутів ліній при малюванні та перетягуванні ручок.
- [x] **[#18] Відображення цінового бейджа на шкалі Y (Price Scale Tag)**: Контрастні бейджі на вертикальній осі цін для активних вузлів.
- [x] **[#19] Інтелектуальний магніт (Smart Magnet Mode)**: Притягування точок малювання до OHLC свічок; інверсія через `Ctrl`.
- [x] **[#20] Додавання зображень та скріншотів до журналу**: Вставка з буфера (`Ctrl+V`), вибір файлу, збереження прев'ю і повнорозмірний перегляд.
- [x] **[#27] Безпечна обробка вставки потокових даних у Panes**: Заміна панік на `log::warn!` при скиданні панелі під час активного запиту.
- [x] **[#28] Безпечна обробка перемикання/сортування індикаторів**: Переведено на `log::error!` з ігноруванням невалідних панелей.
- [x] **[#29] Безпечний Timeframe::to_minutes для субхвилинних таймфреймів**: Повертає `0` для `MS100..MS1000` та додано `try_to_minutes`.
- [x] **[#30] Типізована помилка у біржових адаптерах при невідповідності параметрів**: Повернення `AdapterError::InvalidRequest`.

---

## 2. Критичні дефекти рантайму (P0: Runtime Panics / `todo!` & `unimplemented!`)
*Усі виявлені аварійні гілки та паніки повністю ліквідовані та покриті модульними тестами:*

- [x] **[#13] Обробка аудіо-порогу за обсягом у штуках (Qty Threshold)**
  - **Файл**: `src/modal/audio.rs:407`
  - **Код**: `data::audio::Threshold::Qty(v)`
  - **Статус**: Реалізовано обробку обсягу у штуках/монетах із порівнянням і викликом звуків без паніки процесу.
- [x] **[#14] Безпечна обробка Tick Basis у Comparison Chart**
  - **Файл**: `src/chart/comparison.rs:48, 382`, `src/screen/dashboard/pane.rs:386`
  - **Статус**: Реалізовано безпечний fallback на 1m та ігнорування зміни базису.
- [x] **[#15] Безпечна обробка Tick Basis у Orderbook Heatmap**
  - **Файл**: `src/chart/heatmap.rs:309`, `data/src/aggr/time.rs:653`, `data/src/chart/heatmap.rs:163`
  - **Статус**: Захищено створення `TimeSeries<HeatmapDataPoint>` та `HistoricalDepth` безпечним fallback на 1m.
- [x] **[#16] Обробка Tick Basis в операціях діапазону KlineChart**
  - **Файл**: `src/chart/kline.rs:127`
  - **Статус**: Реалізовано обчислення часових меж свічок на тіковому базисі через `chart.interval_range`.
- [x] **[#27] Небезпечні паніки при вставці потокових даних у Panes**
  - **Файл**: `src/screen/dashboard/pane.rs:405, 484, 517`
  - **Статус**: Замінено аварійні паніки на м'яке логування `log::warn!` та безпечний вихід.
- [x] **[#28] Паніки при перемиканні/сортуванні індикаторів на службових панелях**
  - **Файл**: `src/screen/dashboard/pane.rs:2818, 2830`
  - **Статус**: Переведено на `log::error!` з ігноруванням невалідних дій.
- [x] **[#29] Паніка в Timeframe::to_minutes для субхвилинних таймфреймів**
  - **Файл**: `exchange/src/lib.rs:151`
  - **Статус**: Повертає `0` для `MS100..MS1000` та додано метод `try_to_minutes(self) -> Option<u16>`.
- [x] **[#30] Паніки у біржових адаптерах при невідповідності параметрів**
  - **Файл**: `exchange/src/adapter/binance.rs:817, 824, 986`, `exchange/src/adapter/bybit.rs:638`
  - **Статус**: Повертає `Err(AdapterError::InvalidRequest(...))` замість завершення процесу.

---

## 3. Журнал торгівлі та швидке введення позицій (P1: Trade Journal & Position Autofill)

- [x] **[#25] Швидке автозаповнення журналу торгівлі з графіка (Position Drawing -> Trade Journal Autofill: `Shift + G`)**
  - **Концепція**: Під час побудови фігури `LongPosition` / `ShortPosition` або коли на графіку виділено інструмент позиції, натискання гарячої клавіші `Shift + G` відкриває журнал та автоматично заповнює форму нової угоди.
  - **Параметри автозаповнення**:
    - `Ticker`: активний тікер графіка (наприклад, `BTCUSDT`).
    - `Exchange`: біржа активного графіка (Binance, Bybit, OKX, Hyperliquid).
    - `Direction`: `Long` або `Short` згідно з типом фігури на графіку.
    - `Entry Price`: точка входу з інструменту позиції.
    - `Stop Loss`: рівень стоп-лосу позиції.
    - `Take Profit / Exit Price`: рівень цільового профіту.
    - `Timestamp Open`: часова мітка точки входу.
    - `Notes`: автоматичний розрахунок і запис Risk/Reward (R:R), відстані до стопу у % та до тейка у %.
  - **UI-flow**: Автоматично перемикає сайдбар журналу в активний стан, відкриває форму створення запису з передзаповненими даними та фокусує поле введення розміру або кнопку збереження (`Enter`).

- [ ] **[#21] Автоматичний імпорт угод з бірж за API**
  - Завантаження історії закритих ордерів і розрахунок чистого PnL без ручного введення.

---

## 4. Рефакторинг UX/UI менеджменту лейаутів (P1: Layouts & Workspace Management Overhaul)

- [x] **[#26] Розділення лейаутів та резервного копіювання Workspace**
  - **Проблема поточного UI**:
    - Користувачі плутають дію експорту одного екрана (layout) з повним експортом всієї робочої області (workspace).
    - Кнопка "Open Exports Folder" виглядає зайвою у списку лейаутів і спантеличує користувача.
    - Одночасне виконання трьох дій при експорті (діалог збереження + тихий запис у фонову папку + копіювання JSON у буфер) викликає нерозуміння, куди саме пішов файл.
  - **Архітектурне рішення редизайну**:
    - **Вкладка 1: Лейаути (Layouts)**:
      - Список користувацьких лейаутів із чіткими іконками дій: Перейменувати (`Rename`), Дублювати (`Duplicate`), Видалити (`Delete`).
      - Кнопка: `+ Створити лейаут` (`Add Layout`).
      - Кнопка: `Імпортувати лейаут (.json)` — відкриває діалог вибору файлу одного лейаута.
      - Дія "Експортувати лейаут": чіткий вибір або дія `Зберегти як файл...` без прихованого запису в невідомі папки.
    - **Вкладка 2: Резервне копіювання простору (Workspace Backup & Restore)**:
      - Винесено в окремий логічний блок або підменю.
      - `Створити повний бекап робочого простору` (всі лейаути, теми, списки тікерів, індикатори).
      - `Відновити робочий простір з файлу`.
      - Опція `Відкрити папку з бекапами` розміщується тут із пояснювальним описом.

---

## 5. Кросплатформність: macOS, Windows, Linux (P2: Cross-Platform Support)

- [x] **[#31] Відокремлення залежностей Windows у Cargo.toml та build.rs**
  - Обмежити збірку `winres` виключно для `[target.'cfg(windows)'.build-dependencies]`.
- [x] **[#32] Уніфікація шорткатів для macOS (Command ⌘ vs Control)**
  - У `src/chart/kline.rs` та інших компонентах замінити жорстку прив'язку до `Control` на перевірку `(modifiers.control() || modifiers.command())` для коректної роботи на macOS.
- [x] **[#33] Стабілізація графічного бекенду та системних бібліотек Linux**
  - Перевірка запуску з WGPU на X11 та Wayland.
  - Документування необхідних системних бібліотек для Linux-збірки: `libasound2-dev` (ALSA для rodio), `libfontconfig1-dev`, `libxcb-render0-dev`.
- [x] **[#34] Автоматизація збірки мультиплатформних бінарників (CI/CD)**
  - GitHub Actions матриця для випуску релізів: Windows (`.exe` / `.msi`), macOS (`.dmg` Universal / Apple Silicon & Intel), Linux (`.AppImage` / tar.gz).

---

## 6. Аудит та очищення мертвого/незадіяного коду (P2: Dead Code Elimination)

- [ ] **[#35] Очищення мертвого коду в біржових адаптерах**: *(Заморожено за запитом користувача)*
  - `exchange/src/adapter/hyperliquid.rs`: невикористаний лімітер `HYPERLIQUID_LIMITER`, константи `LIMIT`, `REFILL_RATE`, невикористаний enum `StreamData`, невикористані структури `HyperliquidWSMessage`, `HyperliquidKline`, `HyperliquidDepth`, `HlRecentTradeItem`.
  - `exchange/src/adapter/bybit.rs`: невикористані типи `ApiResponse`, `ApiResult`, `BybitRecentTradeItem`, `BybitRecentTradeResult`, `BybitRecentTradeResponse`.
  - `exchange/src/adapter/binance.rs`: невикористані поля у `FetchedKlines`.
  - `exchange/src/adapter/okex.rs`: невикористані типи `OkxRestTradeItem`, `OkxRestTradeResponse`.
- [ ] **[#36] Очищення мертвого коду в UI та індикаторах**: *(Заморожено за запитом користувача)*
  - `src/chart/indicator/kline/rolling_vwap.rs`: метод `with_window_hours`.
  - `src/chart/indicator/kline/market_pulse.rs`: мертві методи `insert_funding_rates`, `insert_spot_klines`.
  - `src/widget/toast.rs`: невикористані варіанти enum `Status::Secondary`, `Status::Success`.
  - `src/widget.rs`: невикористана константа `DEFAULT_TOOLTIP_DELAY`.
  - `src/chart/indicator/plot/mtm.rs`: незадіяні селектори `stroke_width`, `line_color`, `threshold_color`, `fill_area`.

---

## 7. Розширений функціонал (P3: Advanced Milestones)

- [x] **[#22] Гарячі клавіші для інструментів (T, H, B, R, Esc, Delete)**
- [ ] **[#23] Симуляція кластерів Footprint у режимі Market Replay**
- [ ] **[#24] Шаблони та пресети налаштувань малювання (Drawing Templates)**
