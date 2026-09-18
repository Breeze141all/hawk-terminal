# Hawk Terminal

Desktop-приложение для крипто-чартинга. Поддерживает Binance, Bybit, Hyperliquid, OKX.

Построено на Rust + [Iced](https://github.com/iced-rs/iced).

## Сборка на macOS

### 1. Установи Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

После установки перезапусти терминал или выполни:

```bash
source "$HOME/.cargo/env"
```

### 2. Установи Xcode Command Line Tools

```bash
xcode-select --install
```

### 3. Клонируй и собери

```bash
git clone https://github.com/lubluniky/hawk-client
cd hawk-client
cargo build --release
```

Первая сборка займет несколько минут — скачиваются и компилируются все зависимости.

### 4. Запуск

```bash
cargo run --release
```

Бинарник также будет лежать в `target/release/hawk-terminal`.

## Что умеет

- **Heatmap** — тепловая карта ордербука в реальном времени
- **Candlestick** — свечной график (time-based и tick-based интервалы)
- **Footprint** — кластерный анализ сделок
- **Time & Sales** — лента сделок
- **DOM / Ladder** — стакан с объемами
- **Comparison** — сравнение нескольких тикеров на одном графике
- Звуковые эффекты по потоку сделок
- Мульти-окна / мульти-монитор
- Сохранение лейаутов и кастомные темы

Данные приходят напрямую с публичных API бирж (REST + WebSocket).

## Основано на

[flowsurface-rs/flowsurface](https://github.com/flowsurface-rs/flowsurface) — оригинальный проект
