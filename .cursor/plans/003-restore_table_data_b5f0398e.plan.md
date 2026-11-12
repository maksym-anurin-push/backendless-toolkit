---
name: Restore table data
overview: "Додати `restore_table_data MyTable`: CSV зі stdin → bulk upsert у Console API батчами по 50 (create-or-update за objectId). Без --dry-run."
todos:
  - id: sdk-bulk-upsert
    content: SDK bulk_upsert_records (PUT bulkupsert/{table})
    status: completed
  - id: cli-restore-data
    content: "CLI restore_table_data: CSV parse, coerce, batch upsert 50, tests"
    status: completed
isProject: false
---

# `restore_table_data`

## CLI

```text
cat MyTable.csv | backendless-toolkit-db restore_table_structure MyTable   # existing
cat MyTable.csv | backendless-toolkit-db restore_table_data MyTable
cat MyTable.csv | backendless-toolkit-db restore_table_structure MyTable --apply-required-flag --delete-obsolete-columns # existing
```

- Argv: обов’язкове ім’я таблиці `MyTable`
- Stdin: CSV у форматі `dump_table_data` (header + rows)
- Batch size: **hardcode 50**
- Без `--dry-run`
- Stderr: прогрес (`upserted 50/N …`); stdout порожній при успіху

Усе залишається в одному файлі [`backendless_toolkit_bin/src/db.rs`](backendless_toolkit_bin/src/db.rs) (як домовились для restore structure).

## SDK

У [`backendless_console_sdk/src/tables.rs`](backendless_console_sdk/src/tables.rs) додати:

- `bulk_upsert_records(app_id, table_name, records: &[Value])`  
→ `PUT /{appId}/console/data/bulkupsert/{tableName}` (Console SDK `bulkUpsertRecords`)

## Алгоритм

1. Прочитати CSV зі stdin (`csv` crate).
2. Завантажити schema таблиці (`get_table`) для типів колонок / relations.
3. Для кожного рядка зібрати JSON object:
   - пропустити службові колонки дампу: `___class`, усі `__related__*` (parent-relation counters з Console dump)
   - порожні клітинки → omit (не слати поле)
   - `objectId` якщо є — залишити (для upsert)
   - scalar: coerce за schema `dataType` (`BOOLEAN`, `INT`/`DOUBLE`, `DATETIME` epoch-ms або ISO string як у CSV, інакше string)
   - relation колонки (`DATA_REF` / у `relations`): значення = objectId string (або JSON-масив id для 1:N, якщо клітинка виглядає як list); Backendless приймає id / масив id
4. Бити на чанки по 50 → `bulk_upsert_records`.
5. Помилка API → stderr + non-zero (зупинити наступні батчі).

## Preflight (мінімальний)

- Таблиця `MyTable` існує, інакше fail.
- CSV не порожній і має header.
- Невідомі CSV-колонки (немає в schema і не system skip) → **warn у stderr і skip** (не валити весь restore через зайві поля dump).

## Тести

- Unit: parse/coerce одного рядка CSV → JSON (bool/int/empty/skip `__related__`), chunking на 50.
- Без live мережі.

## Поза scope

- `--dry-run`, configurable batch size
- Окреме відновлення relations через setRelations API (лише ids у тілі upsert)
- Видалення рядків, яких немає в CSV