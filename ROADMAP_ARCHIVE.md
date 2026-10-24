# Hawk Terminal (`hawk-terminal`) — Архів виконаних завдань

Цей документ містить повний історичний реєстр усіх виконаних, протестованих та закритих завдань проєкту Hawk Terminal (релізи `v0.1` – `v0.9.6`).
Синхронізовано з актуальним беклогом: **[ROADMAP.md](ROADMAP.md)**

---

## 1. Деталізований архів завдань релізів v0.8 – v0.9.6

### 1.1. Пріоритет P0: Мережева стабільність, транспорт та паніки (Critical / Stability)

- [x] **[#83] Venue-Aware WebSocket Heartbeats та поділ Read/Write транспорту (EX-13, UP-01)**
  - **Файли**: `exchange/src/adapter/ws.rs`, `exchange/src/adapter/hyperliquid.rs`, `exchange/src/adapter.rs`
  - **Джерело**: Flowsurface `c1388d4`
  - **Проблема**: Спільні тайм-аути читання фреймів у `fastwebsockets` скасовували активні операції зчитування під час надсилання heartbeat ping. На Hyperliquid це розсинхронізовувало парсер фреймів і викликало паніку процесу `Reserved bits are not zero`.
  - **Рішення**: Розділити сокет на `FragmentCollectorRead` та `WebSocketWrite` через `tokio::io::split()`. Фоновий цикл heartbeat не повинен скасовувати або блокувати in-flight зчитування даних. Реалізувати venue-aware політики пінгу для кожного біржового протоколу.


- [x] **[#84] Необмежений буфер WebSocket кадрів із пріоритетним вивантаженням (EX-14, UP-02)**
  - **Файли**: `exchange/src/adapter/ws.rs`, `src/screen/dashboard/pane.rs`
  - **Джерело**: Flowsurface `7a56803`
  - **Проблема**: Канали `mpsc` з фіксованою місткістю переповнювалися під час сильних волатильних спайків та каскадів ліквідацій, що спричиняло втрату трейдів у черзі.
  - **Рішення**: Перевести транспорт на `unbounded_send` з пріоритетним циклом вивантаження кадрів (`MAX_DRAIN_PER_TICK = 256`), гарантуючи обробку 100% тіків без дропів за високого навантаження.


- [x] **[#85] Захист від збою парсингу статистики тікерів Binance Linear/Inverse (EX-15, UP-03)**
  - **Файл**: `exchange/src/adapter/binance.rs:1609-1614`
  - **Джерело**: Flowsurface `b985a9f`
  - **Проблема**: Binance повертає інверсні пари у загальний linear metadata ендпоінт без поля `quoteVolume`. Прямий доступ `item["quoteVolume"].as_str().ok_or_else(...)` падає з помилкою `AdapterError::ParseError("Quote volume not found")`, зриваючи ініціалізацію таблиці тікерів.
  - **Рішення**: Застосувати безпечний фолбек: `serde_util::value_as_f64(&item["quoteVolume"]).or_else(|| serde_util::value_as_f64(&item["volume"]))`.


- [x] **[#86] Міграція маршрутів WebSocket стрімів Binance на оновлені ендпоінти (EX-16, UP-04)**
  - **Файл**: `exchange/src/adapter/binance.rs:548, 727`
  - **Джерело**: Flowsurface `0b2c6ec`
  - **Проблема**: Binance офіційно розділив WS-трафік деривативів: глибина стакана перенесена на окремі маршрути.
  - **Рішення**: Перенаправити потоки біржової глибини `@depth@100ms` на `wss://{domain}/public/stream`, а кліни та угоди (`@aggTrade`, `@kline`) — на `wss://{domain}/market/stream`.


- [x] **[#87] Захист від дедлоку winit при відкритті Popout вікон на Windows (TER-12, UP-05)**
  - **Файли**: `src/window.rs`, `src/screen/dashboard.rs`, `src/screen/dashboard/pane.rs`
  - **Джерело**: Flowsurface `a5df6b2`, [winit#3648](https://github.com/rust-windowing/winit/issues/3648)
  - **Проблема**: В бекенді Iced/winit на Windows одночасний рендеринг кількох вікон спричиняє циклічні зависання черги повідомлень (redraw freeze / GUI hang).
  - **Рішення**: Ввести прапорець `MULTI_WINDOW_SUPPORTED: bool = !cfg!(target_os = "windows")`. На Windows приховати або заблокувати кнопку Popout з попередженням у статус-барі, запобігаючи незворотному зависанню програми.

---


### 1.2. Пріоритет P1: Order Flow, Footprint та Графіка (Core Capabilities)

- [x] **[#88] Індикатор Bar Analysis та табличний режим Footprint Table (TER-13, UP-06)**
  - **Файли**: `src/chart/indicator/kline/bar_analysis.rs`, `data/src/chart/kline.rs`, `src/chart/kline.rs`, `src/modal/pane/settings.rs`
  - **Джерело**: Flowsurface `9d2d8eb`, `ad0ad97`, `1738638`
  - **Концепція**:
    1. **`BarAnalysisIndicator`**: Спеціалізований підпанельний індикатор під графіком, синхронізований із шириною свічок (`cell_width`), який виводить матрицю: **Buy Volume**, **Sell Volume**, **Total Volume**, **Delta ($\Delta$)**, **Delta % ($\Delta\%$)**.
    2. **`ClusterKind::Table`**: Четвертий режим кластера Footprint на додачу до `BidAsk`, `VolumeProfile` та `DeltaProfile`. Відображає чітку сітку з адаптивною шириною під дисбаланси та підтримкою розрахунку nPOC (Naked POC).


- [x] **[#89] Дискове кешування біржових метаданих для миттєвого старту (DAT-14, UP-07)**
  - **Файли**: `data/src/metadata.rs`, `data/src/lib.rs`, `src/screen/dashboard/tickers_table.rs`
  - **Джерело**: Flowsurface `3df72f8`
  - **Концепція**: Збереження структури `metadata-cache.json` з інформацією про тікери, мінімальні розміри лотів (`min_qty`), кроки ціни (`min_ticksize`) та мультиплікатори контрактів. Валідність кешу — 24 години.
  - **Результат**: Запуск Hawk Terminal відбувається за мілісекунди без очікування відповідей від REST API Binance, Bybit, OKX та Hyperliquid. Додавання кнопки «Refresh Metadata» у таблицю тікерів.


- [x] **[#90] Автоматичне успадкування конфігурації при створенні панелей (TER-14, UP-08)**
  - **Файли**: `src/screen/dashboard.rs`, `src/screen/dashboard/pane.rs`
  - **Джерело**: Flowsurface `0bd18d4`, `7d73b7f`
  - **Концепція**: Метод `borrow_synced_settings`. При додаванні або спліті нової панелі вона автоматично переймає візуальні параметри, вибрані дослідження (studies), індикатори та тип кластера з існуючої панелі такого самого типу, усуваючи необхідність повторного ручного налаштування.


- [x] **[#91] Апаратне прискорення Orderbook Heatmap через WGPU Shaders (TER-15, UP-09)**
  - **Файли**: `src/widget/chart/heatmap/`, `Cargo.toml`
  - **Джерело**: Flowsurface `38f49d9`
  - **Концепція**: Заміна CPU Canvas рендерингу (десятків тисяч прямокутників у `Frame`) на повноцінний WGPU-конвеєр з WGSL шейдерами (`heatmap_tex.wgsl`, `rect.wgsl`, `circle.wgsl`):
    - Кільцевий буфер глибини `GridRing` ($4800 \times 2048$).
    - Оновлення тільки брудних стовпчиків (`dirty_cols`).
    - Рендеринг усього теплового стакана за **1 draw call** на GPU, що знижує навантаження на процесор із 80% до <5%.


- [x] **[#92] Конектор історичних угод через Apache Arrow IPC (EX-17, UP-10)**
  - **Файли**: `src/connector/client.rs`, `src/connector/fetcher.rs`, `Cargo.toml`
  - **Джерело**: Flowsurface `14b7551`
  - **Концепція**: Підтримка потокового протоколу `GET /trades.arrow` (Apache Arrow IPC stream: `ts`, `price`, `qty`, `is_sell`). Дозволяє підключати зовнішній локальний сервер на базі DuckDB/Parquet ([flowsurface-server](https://github.com/flowsurface-rs/server)) для необмеженого за часом та швидкісного бекфілу тікових кластерів для всіх бірж без rate-limits.


- [x] **[#21] Автоматичний імпорт угод з бірж за API**
  - **Файли**: `exchange/src/adapter/trade_import.rs`, `data/src/journal.rs`, `src/screen/dashboard/journal.rs`
  - **Концепція**: Завантаження історії закритих ордерів і позицій через біржові API-ключі (Binance, Bybit, OKX) та публічний гаманець (Hyperliquid) з FIFO-реконструкцією позицій, автоматичним розрахунком чистого PnL, O(M+N) дедуплікацією та додаванням у Trade Journal.


- [x] **[#97] Підсистема скриптованих індикаторів `indicator-engine` та каталог .ind (TER-19, UP-14)**
  - **Файли**: `indicator-engine/`, `data/src/chart/kline/indicators/scripting.rs`, `src/screen/dashboard/pane/content/kline/indicator_browser.rs`
  - **Джерело**: Flowsurface `cc8dc08`, `d0b379e`, `5bd5e69`
  - **Концепція**: Інтеграція виділеного крейта `indicator-engine` (парсер, AST, типізація, HIR, bytecode runtime) з підтримкою скриптування індикаторів мовою `.ind`. Автоматична генерація панелей налаштувань (inputs, color pickers, bands). Перенесення каталогу з 36 вбудованих індикаторів без необхідності хардкоду математики та UI у Rust.
  - **Результат**: Двофазний життєвий цикл свічки (`evaluate_closed` + без-алокаційний $O(1)$ `preview_open`), захищений Lyon-рендеринг примітивів, ізоляція від Footprint/Drawings/Replay, браузер індикаторів `indicator_browser.rs` з групуванням за джерелами даних та повною валідацією 480+ тестів.

---


#### 1.3.1. P2.1: Стабільність рантайму, управління пам'яттю та ресурсами (Memory & Stability)

- [x] **[#75] Повноцінна деініціалізація стану при збої REST ресинхронізації стакана (EX-12)**
  - **Файл**: `exchange/src/adapter/binance.rs:347-354`
  - **Проблема**: При помилці отримання знімка стакана прапорець `is_disconnected` не скидався, залишаючи ордербук у напівробочому стані.
  - **Виправлення**: Встановлювати `*is_disconnected = true` та надсилати подію `Event::Disconnected`.


- [x] **[#76] Захист від колізій дескрипторів Windows при збереженні footprint (DAT-11)**
  - **Файл**: `data/src/chart/kline.rs:1810-1830`
  - **Проблема**: Тимчасове блокування файлу кешу сторонніми службами (Windows Defender, Search Indexer) викликає sharing violation під час атомарного перейменування.
  - **Виправлення**: Додати retry-цикл з експоненційною затримкою (3 спроби від 10 мс) перед поверненням помилки вводу-виводу. Покрито тестом `test_dat11_save_daily_footprint_retry_and_write`.


- [x] **[#77] Оптимізація виділення пам'яті в check_kline_integrity (DAT-12)**
  - **Файл**: `data/src/aggr/time.rs:182-187`
  - **Проблема**: Наперед виділяється до 500 МБ пам'яті під масив пропущених інтервалів навіть тоді, коли пропусків у свічках немає.
  - **Виправлення**: Зменшити стартову місткість вектора до `Vec::with_capacity(std::cmp::min(64, range))` без надлишкових алокацій.


- [x] **[#78] Заміна O(N) зсуву пам'яті на VecDeque у LiquidationHeatmap (DAT-13)**
  - **Файл**: `data/src/chart/liquidation_heatmap.rs:155-255`
  - **Проблема**: `Vec::remove(0)` призводить до копіювання всього масиву бакетів при кожному оновленні сітки.
  - **Виправлення**: Замінити внутрішні буфери (`price_ranges`, `buy_vol_buffer`, `sell_vol_buffer`) на `std::collections::VecDeque` з методом `pop_front()`, ліквідувавши копіювання $O(N)$ на кожен бар.


- [x] **[#81] Обмеження розміру історії тікових свічок (TER-10)**
  - **Файли**: `data/src/aggr/ticks.rs:96-103`, `data/src/aggr/time.rs:328-333, 568-573`
  - **Проблема**: Вектор свічок монотонно зростає при тривалих робочих сесіях, призводячи до витоку оперативної пам'яті.
  - **Виправлення**: Обмежувати довжину кешу через `drain` при перевищенні порогу 10 000 елементів у `TickAggr` та викликати `enforce_retention_limit()` у realtime-потоках свічок (`insert_realtime_trades`, `insert_trades_or_create_bucket`). Покрито тестом `test_ter10_timeseries_realtime_trades_retention_limit`.


- [x] **[#102] Придушення дублікатів Toast-повідомлень та експоненційний бекофф помилок (TER-22, UP-19)**
  - **Файли**: `src/notify.rs`, `src/widget/toast.rs`, `src/main.rs`
  - **Джерело**: Flowsurface `b90218d`
  - **Концепція**: Дедуплікація спливаючих сповіщень однакового типу (вікно 10 с), агрегація лічильника повторень (retry count) замість накопичення черги з десятків однакових повідомлень, експоненційний інтервал затримки (30-300 с) для повторюваних мережевих помилок `Status::Danger`. Покрито тестами `test_toast_suppression_and_deduplication`, `test_error_aggregation_format`.


#### 1.3.2. P2.2: Обчислювальна оптимізація та архітектура шкали (Performance & Engine)

- [x] **[#82] Діапазонний скан замість O(N) для розрахунку VWAP (TER-11)**
  - **Файл**: `src/chart/kline.rs`
  - **Проблема**: На кожному кадрі розраховувалася вся історія свічок замість видимого вікна ($O(N)$ overhead).
  - **Виправлення**: Застосовано `ts.datapoints.range(earliest..=latest)` та `range(fetch_earliest..=latest)` для $O(\log N + K)$ діапазонного сканування та бінарний пошук `partition_point` для `TickBased`. Покрито тестом `test_ter11_vwap_range_scan_datapoints`.


- [x] **[#94] Модульна реструктуризація розрахунку координат і шкали (DAT-15, UP-12)**
  - **Файли**: `data/src/chart/ticks/`, `src/chart/ticks/`, `src/chart/scale.rs`, `data/src/config/timezone.rs`
  - **Джерело**: Flowsurface `12b029c`
  - **Концепція**: Заміна старого монолітного `scale.rs` на спеціалізовані модулі `ticks/x.rs` та `ticks/y.rs` із жорстким бюджетом щільності (`MAX_GRID_LINES = 1000`, `Y_LABEL_DENSITY`), що запобігає накладанню цінових та часових міток. Додано календарні методи вирівнювання сітки в `data/src/config/timezone.rs`, розширено арифметику `Price` та `UnixMs`, збережено беквард-сумісний шим `src/chart/scale.rs`.


- [x] **[#101] TradeTpoCache: субсекундний розрахунок TPO із потоку сирих угод (DAT-16, UP-18)**
  - **Файли**: `data/src/chart/tpo.rs`, `src/screen/dashboard/pane.rs`, `src/chart/kline.rs`, `src/profile/tpo.rs`
  - **Джерело**: Flowsurface `0a0416b`
  - **Концепція**: Розширення існуючого TPO модуля Hawk Terminal: додавання структури `TradeTpoCache` для інкрементального розрахунку профілів безпосередньо з потоку сирих угод `Trade` з календарною прив'язкою `TpoAnchor` (Daily/Weekly/Monthly) та 1-хвилинним базисом (`OneMinBar`) для точного формування літерних періодів при низькій ліквідності або розривах у свічках. Суб-мілісекундне виконання (<20 мс для 25k угод), збереження екстеншнів Initial Balance, Point of Control (POC), Value Area (VA) та Single Prints. Інтегровано реєстрацію подвійного стріму (Kline + DepthAndTrades) у `src/screen/dashboard/pane.rs` та злиття у `prepare_tpo_profiles`. Покрито модульним набором тестів `test_trade_tpo_cache_*`.


#### 1.3.3. P2.3: Біржові адаптери та нові ринки (Exchange Integrations)

- [x] **[#95] Інтеграція біржового адаптера MEXC Spot & Futures (EX-18, UP-13)**
  - **Файли**: `exchange/src/adapter/mexc.rs`, `exchange/src/adapter.rs`, `src/style.rs`, `src/screen/dashboard.rs`, `src/screen/dashboard/tickers_table.rs`
  - **Джерело**: Flowsurface `5299635`, `6bf4bf9`
  - **Концепція**: Портування адаптера біржі MEXC: WebSocket ринкові стріми (`push.deal`, `push.depth`, `push.kline`), REST клієнт метаданих/статистики/свічок/снепшотів глибини стакана, підтримка лінійних/інверсних ф'ючерсів і споту, перевірка лімітів швидкості запитів (`MexcLimiter`), фільтрація таймфреймів та синхронізація годинника. Покрито тестами `test_mexc_*`.


- [x] **[#99] Підтримка Hyperliquid HIP-3 DEXes та пар із котируванням у USDC (EX-19, UP-16)**
  - **Файли**: `exchange/src/adapter/hyperliquid.rs`, `exchange/src/lib.rs`
  - **Джерело**: Flowsurface `1e039db`, `54403f1`, `f81677b`
  - **Концепція**: Підтримка синтетичних дексів та builder token контрактів Hyperliquid HIP-3 (`xyz:*`, `flx:*`, `vntl:*`). Коректна десеріалізація тікерів із двокрапками через `split_once(':')`, підтримка пар із котируванням у USDC без примусового суфіксу USDT, оновлення REST-запитів метаданих (`perpDexs`, `spotMetaAndAssetCtxs`) для точної ідентифікації базових і котирувальних активів. Покрито тестами `test_hyperliquid_hip3_dex_ticker_serde` та `test_hyperliquid_ticker_display_no_forced_usdt`.


#### 1.3.4. P2.4: UX, Order Flow аналітика та інструменти трейдингу (UI/UX & Trading Tools)

- [x] **[#79] Відновлення сортування таблиці тікерів при збої мережевого запиту (TER-08)**
  - **Файл**: `src/screen/dashboard/tickers_table.rs:242-279`
  - **Проблема**: Збій одного запиту статистики залишає лічильник `pending_stats_batches > 0`, що назавжди блокує функцію сортування в UI.
  - **Виправлення**: Декрементувати лічильник незавершених батчів в гілці обробки помилок `Action::ErrorOccurred`. Покрито тестом `test_ter08_tickers_table_sort_recovery_on_fetch_error`.


- [x] **[#80] Асинхронний вибір файлів зображень у журналі без блокування UI (TER-09)**
  - **Файли**: `src/journal_media.rs:83-91`, `src/screen/dashboard/journal.rs:413-415`
  - **Проблема**: Синхронний нативний діалог вибору файлу повністю заморожує головний потік рендерингу графіків.
  - **Виправлення**: Перевести вибір зображень на асинхронний `rfd::AsyncFileDialog::new().pick_file().await` без блокування головного потоку GUI. Покрито тестом `test_ter09_async_file_dialog_future`.


- [x] **[#93] Форматування Time & Sales за UserTimezone та zero-alloc буфер (TER-16, UP-11)**
  - **Файли**: `src/screen/dashboard/panel/timeandsales.rs`, `data/src/panel/timeandsales.rs`
  - **Джерело**: Flowsurface `1e30016`
  - **Концепція**: Застосувати `self.timezone.format_with_kind(...)` замість жорстко закодованого UTC. Видалити структуру `TradeEntry` з heap `String` аллокаціями, перейшовши на безпосереднє збереження та рендеринг `Trade` через без-алокаційний `TapeTrade`. Покрито тестом `test_ter16_timeandsales_zero_alloc_buffer_and_tz`.


- [x] **[#98] Мульти-тікерний режим агрегації стрічки угод Time & Sales (TER-20, UP-15)**
  - **Файли**: `data/src/panel/timeandsales.rs`, `src/screen/dashboard/panel/timeandsales.rs`, `src/screen/dashboard/pane.rs`, `src/screen/dashboard.rs`
  - **Джерело**: Flowsurface `5077a2f`
  - **Концепція**: Можливість додавати кілька інструментів/бірж до однієї панелі Time & Sales із паралельною підпискою на ринкові потоки угод. Візуальна кольорова диференціація тікерів, хронологічне сортування загального потоку через бінарний пошук `partition_point` ($O(\log N)$) без пропусків та фільтрація окремих символів у реальному часі. Покрито тестами `test_ter20_multi_ticker_chronological_ordering` та `test_ter20_multi_ticker_config_defaults`.


- [x] **[#100] Дослідження Big Trades із логарифмічним масштабуванням та номіналом у USD (TER-21, UP-17)**
  - **Файли**: `data/src/chart/kline/big_trades.rs`, `src/widget/chart/big_trades.rs`, `src/chart/kline.rs`, `src/modal/pane/settings.rs`
  - **Джерело**: Flowsurface `0a0416b`
  - **Концепція**: Спеціалізоване дослідження для Footprint/Kline графіка: фільтрація угод за порогом номіналу (наприклад, > $100k, > $1M), підрахунок обсягу у валюті котирування, логарифмічний розрахунок радіуса маркерів угод $r = \text{clamp}(r_{\min} + \text{scale} \cdot \ln(1 + \frac{\text{usd} - \text{min}}{\text{min}}), r_{\min}, r_{\max})$, бінарний пошук діапазонів часу через `partition_point` ($O(\log N)$) для підтримки 120+ FPS під час скролу та зуму. Покрито тестами `test_ter21_big_trades_logarithmic_radius_scaling` та `test_ter21_big_trades_partition_point_binary_search`.


- [x] **[#24] Шаблони та пресети налаштувань малювання (Drawing Templates)**
  - **Файли**: `data/src/chart/drawing.rs`, `data/src/lib.rs`, `src/chart/kline.rs`, `src/widget/chart/drawing_selection_toolbar.rs`, `src/screen/dashboard/pane.rs`
  - **Концепція**: Збереження стилів (колір, товщина лінії, прозорість, параметри позиції) у користувацькі шаблони `DrawingTemplate` та дисковий кеш `drawing-templates.json`. Швидке застосування вбудованих пресетів (Cyan Accent, Key Level, Major Alert, Faint Guide, Accent Zone) безпосередньо з панелі виділення фігури з повною підтримкою undo/redo та синхронізації між графіками. Покрито тестами `test_drawing_template_application`, `test_drawing_template_store_crud_and_serialization`, `test_task24_drawing_templates_application_and_presets`.

---


---

## 2. Архів реалізованих завдань (Done & Verified Archive)

### 2.1. Торговий функціонал, Order Flow та Графіка

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
- [x] **[#17] Фіксація кута 45° при затиснутому Shift (45° Angle Snap)**: Прив'язка кутів ліній при малюванні та перетягуванні ручок.
- [x] **[#18] Відображення цінового бейджа на шкалі Y (Price Scale Tag)**: Контрастні бейджі на вертикальній осі цін для активних вузлів.
- [x] **[#19] Інтелектуальний магніт (Smart Magnet Mode)**: Притягування точок малювання до OHLC свічок; інверсія через `Ctrl`.
- [x] **[#20] Додавання зображень та скріншотів до журналу**: Вставка з буфера (`Ctrl+V`), вибір файлу, збереження прев'ю і повнорозмірний перегляд.
- [x] **[#22] Гарячі клавіші для інструментів (T, H, B, R, Esc, Delete)**.
- [x] **[#23] Симуляція кластерів Footprint у режимі Market Replay**: Реалізовано `KlineTrades::simulate_from_kline` у `hawk-data` з гауссівським розподілом об'ємів при відсутності сирих тіків.
- [x] **[#25] Швидке автозаповнення журналу торгівлі з графіка (Position Drawing -> Trade Journal Autofill: `Shift + G`)**: Перенесення параметрів активної позиції, напрямку, ціни входу, SL, TP, R:R та відкриття форми журналу.
- [x] **[#26] Мінімалістичний експорт та імпорт лейаутів**: В один клік у системну папку `Downloads` із регенерацією `pane_id` та захистом від колізій.
- [x] **[#47] Система Undo / Redo для малювань (Ctrl+Z / Ctrl+Y / Cmd+Z)**: Стек історії `DrawingHistoryAction` на 100 операцій, підтримка кирилиці (`я`, `н`), багаторівневий відкат дій.

---

### 2.2. Ліквідація критичних рантайм-панік та дефектів даних

- [x] **[#13] Обробка аудіо-порогу за обсягом у штуках (Qty Threshold)**: Повний розрахунок обсягу покупок/продажів без паніки процесу (`src/modal/audio.rs`).
- [x] **[#14] Безпечна обробка Tick Basis у Comparison Chart**: Fallback на 1m таймфрейм та ігнорування непідтримуваної зміни без крашу.
- [x] **[#15] Безпечна обробка Tick Basis у Orderbook Heatmap**: Безпечний fallback для TimeSeries та HistoricalDepth на 1m.
- [x] **[#16] Обробка Tick Basis в операціях діапазону KlineChart**: Розрахунок діапазону свічок через `interval_range` на тіковому базисі.
- [x] **[#27] Безпечна обробка вставки потокових даних у Panes**: Заміна панік на `log::warn!` при скиданні панелі під час активного запиту.
- [x] **[#28] Безпечна обробка перемикання/сортування індикаторів**: Переведено на `log::error!` з ігноруванням невалідних панелей.
- [x] **[#29] Безпечний Timeframe::to_minutes для субхвилинних таймфреймів**: Повертає `0` для `MS100..MS1000` та додано `try_to_minutes`.
- [x] **[#30] Типізована помилка у біржових адаптерах при невідповідності параметрів**: Повернення `AdapterError::InvalidRequest`.
- [x] **[#37] Відкладений рендеринг динамічного трекінгу позиції (TradingView-style)**: Зони TP/SL і R:R видимі завжди, динамічний PnL малюється лише після фактичного входу (`outcome.is_entered == true`).
- [x] **[#38] Фантомний layout "Hawk" після перезапусків**: Виправлено форсоване впровадження дефолтної конфігурації в `src/layout.rs`.
- [x] **[#39] Алерти не надсилають Windows-нотифікації**: Додано фонові системні сповіщення через `notify-rust` (WinRT Toast API).
- [x] **[#40] Volume indicator увімкнений за замовчуванням**: Вилучено примусовий індикатор об'єму при відкритті нових графіків.
- [x] **[#41] Некоректна агрегація Footprint на 1h таймфреймі**: Атомарний комміт пакетів угод через `commit_fetched_trades` лише після завершення батчу.
- [x] **[#42] Footprint може не рендеритись коли програма не у фокусі**: Fallback до останніх 60 свічок або поточного часу при нульовій геометрії вікна.
- [x] **[#43] Size/Volume: відкривати як вбудовану панель, а не окреме вікно**: Режим `VolumeSearch` у головній області без створення popout.
- [x] **[#44] Price alerts: обмежити переміщення тільки за handle**: Прапорець `alert_drag_handle_only` та перехоплення drag виключно за бейдж цінової шкали.
- [x] **[#45] Рефакторинг UI налаштувань**: 2-колонковий лейаут 560px із лівим категорійним сайдбаром.
- [x] **[#48] Синхронізація часу (Clock Skew Synchronization)**: Алгоритм Крістіана з вимірюванням RTT та періодичним біржовим пінгом для захисту від блокування свічок.
- [x] **[#49] Захист від бібліотечного аборту через std::process::exit(1) (EX-01)**: `AdapterError::InvalidRequest` замість виходу при HTTP 429/418/403.
- [x] **[#50] Усунення розсинхрону бінарного пошуку в Heatmap за сирою ціною (DAT-01)**: Бінарний пошук виключно за `grouped_price`.
- [x] **[#51] Виправлення фальсифікації першого торкання Naked POC (DAT-02)**: Точний хронологічний скан свічок без шорткатів екстремумів.
- [x] **[#52] Обмеження монотонного зростання пам'яті (OOM) у буферах свічок (DAT-03)**: Ліміт `MAX_RETAINED_DATAPOINTS = 10_000` з витісненням застарілих даних.
- [x] **[#53] Захист від паніки аллокатора при десеріалізації кешу .fst (DAT-04)**: Попередня валідація місткості `count <= (buf.len() - 10) / MIN_CANDLE_HEADER_BYTES`.
- [x] **[#54] Безпечний доступ до першого елемента tickers у панелях графіків (TER-01)**: Перевірка `let Some(&base_ticker) = tickers.first()`.
- [x] **[#55] Безпечне порівняння координат у вимірювальних інструментах (TER-02)**: Захист `.unwrap()` через `partial_cmp().unwrap_or(Equal)`.
- [x] **[#56] Захист від ділення на нуль та від'ємної геометрії в Canvas footprint (TER-03)**: Валідація `max_cluster_qty > 0.0` та скінченності ширини барів.
- [x] **[#57] Обробка помилок Binance aggTrades без тихої втрати даних (EX-04)**: Повернення `Err(AdapterError::InvalidRequest(...))` при збоях замість порожнього списку.
- [x] **[#58] Запобігання спін-циклам 100% CPU при збої WebSocket фреймів (EX-06)**: Асинхронний sleep 1с перед переходом у `State::Disconnected`.
- [x] **[#59] Періодичний скид накопичених трейдів (Flush Timer) для повільних ринків (EX-07)**: Інтервал 30 мс у циклі `tokio::select!` адаптерів Bybit, OKX та Hyperliquid.
- [x] **[#60] Валідація тікера в мультиплексному демультиплексорі Binance kline (EX-08)**: Строга перевірка відповідності пари та інтервалу перед обробкою.
- [x] **[#61] Відновлення SingleFlight при падінні таски лідера (EX-10)**: Повторний запуск запиту наступним фоловером при падінні лідера.
- [x] **[#62] Усунення нескінченного циклу бекфілу на закритих історичних днях (DAT-05)**: Маркування `trades_fetched = true` для закритих днів.
- [x] **[#63] Атомарний запис конфігураційних файлів через тимчасові файли (DAT-07)**: Запис у `.tmp.<pid>` файл з наступним атомарним перейменуванням `fs::rename`.
- [x] **[#64] Маршрутизація подій зміни сітки у винесених вікнах Popout (TER-06)**: Ізоляція обробників ресайзу та закриття на контейнері `popout`.
- [x] **[#65] Захист від нескінченного циклу бекфілу при отриманні порожнього батчу трейдів (TER-07)**: Маркування кінця діапазону при `next_trade_t <= latest_trade_t`.
- [x] **[#66] Безпечний парсинг CSV архівів щоденних трейдів Binance (EX-02)**: Безпечне зчитування колонок через `record.get(5)?.parse()`.
- [x] **[#67] Захист від паніки SystemTimeError при зворотній синхронізації годинника (EX-03, TER-05)**: Захист системного часу через `unwrap_or_default()`.
- [x] **[#68] Захист від u64 underflow при зворотному скролі/зумі OKX (EX-05, TER-04)**: Розрахунок меж через `saturating_sub()`.
- [x] **[#69] Очищення застарілих блокувань у TradeCoordinator та RequestHandler (EX-09)**: Метод `prune_old_locks` для записів старших 2 днів.
- [x] **[#70] Винесення LZ4 компресії та дискового запису трейдів з потоків Tokio (EX-11)**: Загортання синхронного запису в `tokio::task::spawn_blocking`.
- [x] **[#71] Захист глобальних RwLock від отруєння та асинхронне збереження (DAT-06)**: Стійкість до poisoning через `unwrap_or_else(|p| p.into_inner())`.
- [x] **[#72] Безпечне розбиття рядків UTF-8 за границями символів у таблиці тікерів (DAT-08)**: Використання `char_indices()` для визначення байтових меж.
- [x] **[#73] Захист від Integer Underflow при max_batch == 0 у слайсі кешу (DAT-09)**: Попередня перевірка нульового батчу.
- [x] **[#74] Збереження валідного кешу угод для неліквідних та нещодавно створених пар (DAT-10)**: Збереження кешу при низькій активності пари.
- [x] **[#83] Venue-Aware WebSocket Heartbeats та поділ Read/Write транспорту (EX-13, UP-01)**: Розділення `fastwebsockets` на `FragmentCollectorRead` та `WebSocketWrite` через `tokio::io::split()`, venue-aware ping policy без cancellation hazard.
- [x] **[#84] Необмежений буфер WebSocket кадрів із пріоритетним вивантаженням (EX-14, UP-02)**: Пакетне вивантаження `drain_unbounded_batch` з лімітом 256 кадрів/тік для ліквідації втрат під час спайків волатильності.
- [x] **[#85] Захист від збою парсингу статистики тікерів Binance Linear/Inverse (EX-15, UP-03)**: Безпечний fallback `quoteVolume` -> `volume` при відсутності котирувального обсягу в інверсних контрактах.
- [x] **[#86] Міграція маршрутів WebSocket стрімів Binance на оновлені ендпоінти (EX-16, UP-04)**: Розділення біржових потоків: `@depth` на `/public/stream`, а `@aggTrade` та `@kline` на `/market/stream`.
- [x] **[#87] Захист від дедлоку winit при відкритті Popout вікон на Windows (TER-12, UP-05)**: Прапорець `MULTI_WINDOW_SUPPORTED = !cfg!(target_os = "windows")`, блокування popout-дії та статус-бар попередження на Windows.
- [x] **[#75] Повноцінна деініціалізація стану при збої REST ресинхронізації стакана (EX-12)**: Встановлення `*is_disconnected = true` та надсилання `Event::Disconnected` при невдалому отриманні snapshot стакана.
- [x] **[#76] Захист від колізій дескрипторів Windows при збереженні footprint (DAT-11)**: Retry-цикл з експоненційною затримкою (3 спроби від 10 мс) під час атомарного перейменування файлу кешу.
- [x] **[#77] Оптимізація виділення пам'яті в check_kline_integrity (DAT-12)**: Зменшення попереднього виділення пам'яті до `Vec::with_capacity(min(64, range))` замість монолітного розміру всього інтервалу.
- [x] **[#78] Заміна O(N) зсуву пам'яті на VecDeque у LiquidationHeatmap (DAT-13)**: Заміна буферів на `VecDeque<f32>` з `pop_front()`, що ліквідувало копіювання пам'яті на кожен новий бар.
- [x] **[#81] Обмеження розміру історії тікових свічок (TER-10)**: Ліміт `MAX_RETAINED_DATAPOINTS = 10_000` з витісненням через `drain` у `TickAggr` та примусова підтримка ліміту в realtime-потоках свічок.
- [x] **[#102] Придушення дублікатів Toast-повідомлень та експоненційний бекофф помилок (TER-22, UP-19)**: Модуль `notify::Notifications` з дедуплікацією інфо-повідомлень (10 с), експоненційним бекоффом помилок (30-300 с) та агрегацією лічильника ретраїв.

---

### 2.3. Кросплатформність та автоматизація релізів

- [x] **[#31] Відокремлення залежностей Windows у Cargo.toml та build.rs**: Обмеження збірки `winres` виключно для цільової ОС Windows.
- [x] **[#32] Уніфікація шорткатів для macOS (Command ⌘ vs Control)**: Підтримка `(modifiers.control() || modifiers.command())` на всіх чартах.
- [x] **[#33] Стабілізація графічного бекенду та системних бібліотек Linux**: Інтеграція `libasound2-dev`, `libfontconfig1-dev`, `libxcb-render0-dev`.
- [x] **[#34] Автоматизація збірки мультиплатформних бінарників (CI/CD)**: Матриця GitHub Actions для Windows (`.exe` інсталятор Inno Setup), macOS (`.tar.gz`), Linux (`.tar.gz`).
