# AGENTS.md — TeivrimSite (anime_db)

> **Сначала прочитай `D:\PROJECT_FOLDER\AUTOPILOT\DOCTRINE.md` целиком.**
> Это приказ, который действует всегда. Без него работать нельзя.

## Проект

Аниме-каталог: агрегатор AniList + Kitsu + Shikimori, JSON API + статический веб-фронтенд.
Rust 2021, actix-web 4.15, rusqlite (SQLite, bundled), reqwest/rustls.
Крейт `anime_db` v2.0.0, бинарь `anime_db` (точка входа — `default-run`).

```
src/
  main.rs        точка входа, конфиг, запуск HTTP-сервера
  config.rs      env-конфигурация
  error.rs       типы ошибок
  models.rs      модели предметной области
  upstream.rs    клиенты внешних API (AniList/Kitsu/Shikimori)
  api/           HTTP-обработчики
  db/            SQLite: пул, миграции, запросы
  http/          middleware, CORS, статика
  loader/        фоновые загрузчики данных
  sources/       адаптеры источников
frontend/        статический фронтенд (без сборщика)
android/         Android-обёртка
tools/           PowerShell-скрипты: иконки, keystore, RuStore
```

## Верификация (единственный источник правды)

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File D:\PROJECT_FOLDER\AUTOPILOT\verify.ps1 -Project TeivrimSite
```

Что она запускает:
- `cargo build --offline --all-targets`
- `cargo test --offline`
- `cargo clippy --offline --all-targets -- -D warnings`

**Exit 0 — можно идти дальше. Exit ≠ 0 — ты не имеешь права завершиться.**

## Известные долги (проверено 2026-09-28)

- **Тестов нет вообще.** Ни одного `#[test]` в `src/`. Это дыра номер один.
  Начни с `upstream.rs` (парсинг и маппинг ответов) и `models.rs`.
- Собирай **только с `--offline`**: все зависимости запиннены и лежат в локальном
  кэше cargo. Без флага уйдёшь в сеть и упадёшь без интернета.
- `data\smoke.db` (31 МБ) и `-wal` — это артефакт прогона. Не коммить, не удаляй
  чужое, но и в репозиторий не тащи.

## Правила проекта

- Зависимости **не добавляй** без крайней необходимости: политика проекта — минимум
  зависимостей, всё, что можно, написано вручную (см. комментарий в `Cargo.toml`).
  Если добавляешь — обнови комментарий «Removed on purpose».
- CORS, статика, пул — написаны руками. Не тяни `actix-cors`, `actix-files`, `deadpool`.
- Миграции — только вперёд, только через `src/db/`.
- Внешние данные (AniList/Kitsu/Shikimori) **никогда** не разворачивай через `unwrap()`.
  Это главный источник падений в рантайме.
