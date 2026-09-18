# Hawk Terminal (`hawk-client`) — Product Roadmap & Task Tracker

Система обліку задач синхронізована з GitHub Issues репозиторію:
**[Breeze141all/ct-client Issues](https://github.com/Breeze141all/ct-client/issues)**

---

## 1. Завершено (Done / Implemented & Verified)
*Всі задачі повністю реалізовані в коді та проходять 116/116 модульних тестів.*

- [x] **[#1] 2D Liquidation Heatmap** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/1)): 2D сітка Time × Price, квантування бінів ($100 для BTC), відсікання тінями свічок, палітри тем.
- [x] **[#2] Multi-Period Rolling VWAP** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/2)): Ковзні вікна 7d, 30d, 90d, 365d та сесійні періоди з смугами 1, 2, 3 SD.
- [x] **[#3] Drawing Suite** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/3)): Trendline, Ray, Horiz Line, Rectangle, Fibonacci Retracement, Text Note.
- [x] **[#4] Market Replay Engine** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/4)): Покрокове відтворення, інтерактивний календар вибору дати/часу, Random Bar.
- [x] **[#5] Price Alerts** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/5)): Рівні CrossAbove/Below, аудіо-сигнали, візуальні Toasts, бейджі на осі Y, менеджер алертів.
- [x] **[#6] Trade Journal** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/6)): Облік торгів, PnL, 3 режими (Disabled, Basic Sidebar 330px, Extended Fullscreen Dashboard).
- [x] **[#7] TPO Multi-Period & Session Merge** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/7)): Daily/Weekly/Monthly агрегація, ручне злиття/розділення сесій, літери понад 52.
- [x] **[#8] Footprint Binary Cache & RAM Capping** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/8)): LZ4 сирі угоди, `.fp.bin` кеш кластерів, лімітування пам'яті, чесний backfill.
- [x] **[#9] Order Flow Indicators** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/9)): CVD, Bid/Ask Ratio, Position Flow, Net OI.
- [x] **[#10] Workspace Bundle** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/10)): Експорт/імпорт налаштувань та лейаутів з захистом від DoS.

---

## 2. У процесі (In Progress)

- [ ] **[#11] Розбивка 43 незбережених файлів на атомарні коміти в Git** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/11)):
  - Стан: Робоче дерево містить 43 модифікованих і 17 нових файлів.
  - Ціль: Очистити робоче дерево шляхом створення 6 логічних комітів і пушу в `origin/main`.

---

## 3. Заплановано (Backlog / Future Milestones)

- [ ] **[#12] Автоматичний імпорт угод з API бірж у Trade Journal** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/12)).
- [ ] **[#13] Гарячі клавіші для інструментів малювання (T, H, F, B, Esc)** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/13)).
- [ ] **[#14] Симуляція Footprint Delta та кластерів у режимі Market Replay** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/14)).
- [ ] **[#15] Збереження пресетів та стилів малювання (Templates)** ([Деталі на GitHub](https://github.com/Breeze141all/ct-client/issues/15)).
