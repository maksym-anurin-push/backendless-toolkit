---
name: Restore data bulk flag
overview: Повернути bulk upsert для restore_table_data за прапорцем --bulk (батчі по 50); без прапорця лишити поточну поштучну create/update логіку.
todos: []
isProject: false
---

# `restore_table_data --bulk`

```shell
cat MyTable.csv | ./target/debug/backendless-toolkit-db restore_table_data MyTable
cat MyTable.csv | ./target/debug/backendless-toolkit-db restore_table_data --bulk MyTable
```

- Без `--bulk` — як зараз: на рядок `record_exists` → `put_record` / `create_record` (з password-хаком для `Users`).
- З `--bulk` — `PUT …/console/data/bulkupsert/{table}` чанками по **50** (`UPSERT_BATCH_SIZE`).
- Прапорець дозволений у будь-якій позиції серед argv після команди; обов’язкове одне ім’я таблиці.
- Для `Users` + `--bulk` — одразу fail з підказкою використати режим без `--bulk` (відомий API 3057 на create через bulkupsert).

## Зміни

### SDK — [`backendless_console_sdk/src/tables.rs`](backendless_console_sdk/src/tables.rs)

Розкоментувати / відновити `bulk_upsert_records` → `PUT {appId}/console/data/bulkupsert/{tableName}` з тілом JSON-масиву записів.

### CLI — [`backendless_toolkit_bin/src/db.rs`](backendless_toolkit_bin/src/db.rs)

1. Парсер argv для `restore_table_data`: `(table, bulk: bool)`.
2. Оновити `usage` / module docs.
3. `restore_table_data(..., bulk: bool)`:
   - спільний CSV → `records` (як зараз);
   - `bulk == false` — існуючий цикл;
   - `bulk == true` — strip `updated` на кожному записі (як у per-row), `chunk_records(..., 50)`, `bulk_upsert_records`, stderr `upserted {done}/{total}`.
4. Зняти `#[allow(dead_code)]` з `UPSERT_BATCH_SIZE` / `chunk_records` коли bulk знову живий.

### Docs — [`README.md`](README.md)

Два приклади: без і з `--bulk`.

### Тести

- Парсинг argv: `--bulk MyTable`, `MyTable --bulk`, без прапорця, зайві args → err.
- Існуючий `chunk_records_batches_by_fifty` лишається.