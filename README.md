[![Build Status](???.svg)](???)

# Backendless Toolkit

Backendless Toolkit provides CLI tools and MCP server to manage Backendless Applications via [Backendless REST API Documentation](https://backendless.com/docs/rest/)

## Development

1. Configure [credential provider](https://doc.rust-lang.org/cargo/reference/registry-authentication.html)
1. Create [access token](https://gitea.anurin.name/user/settings/applications) with **Package Read** permission
1. Authorize into registry
   ```shell
   TOKEN=abc123
   echo -n "Bearer ${TOKEN}" | cargo login --registry freemework
   ```
1. Work with code

```shell
cargo clean
cargo build --locked
cargo test --frozen
cargo run --frozen --package grai_registry_bin -- --backendless.1.url=https://eu-api.backendless.com --backendless.2.url=https://api-bl-k8s.baelab.net 
```

## Database CLI

Configure Console credentials in `.env` (see `.env-example`), then:

```shell
cargo build --locked --bin backendless-toolkit-db
./target/debug/backendless-toolkit-db show_tables
./target/debug/backendless-toolkit-db show_tables PTInstallmentTemplateMapping KycUpdateStatus CmsUser
./target/debug/backendless-toolkit-db dump_table_structure MyTable
./target/debug/backendless-toolkit-db dump_table_data MyTable
cat MyTable.yaml | ./target/debug/backendless-toolkit-db restore_table_structure MyTable --dry-run
cat MyTable.yaml | ./target/debug/backendless-toolkit-db restore_table_structure MyTable
cat MyTable.yaml | ./target/debug/backendless-toolkit-db restore_table_structure MyTable --apply-required-flag --delete-obsolete-columns
cat MyTable.csv | ./target/debug/backendless-toolkit-db restore_table_data MyTable
cat MyTable.csv | ./target/debug/backendless-toolkit-db restore_table_data --bulk MyTable
```

## Test 

### Test 1

```shell
BACKENDLESS_TOOLKIT__CONSOLE__APP_ID=BAF3E974-52AA-7598-FF04-56945EF93500 ./target/debug/backendless-toolkit-db dump_table_data CorporateAppModule > CorporateAppModule.csv
BACKENDLESS_TOOLKIT__CONSOLE__APP_ID=BAF3E974-52AA-7598-FF04-56945EF93500 ./target/debug/backendless-toolkit-db dump_table_data CorporateApprovalLevel > CorporateApprovalLevel.csv
BACKENDLESS_TOOLKIT__CONSOLE__APP_ID=BAF3E974-52AA-7598-FF04-56945EF93500 ./target/debug/backendless-toolkit-db dump_table_data CorporateApprovalPermission > CorporateApprovalPermission.csv
BACKENDLESS_TOOLKIT__CONSOLE__APP_ID=BAF3E974-52AA-7598-FF04-56945EF93500 ./target/debug/backendless-toolkit-db dump_table_data CorporateEmployee > CorporateEmployee.csv
BACKENDLESS_TOOLKIT__CONSOLE__APP_ID=BAF3E974-52AA-7598-FF04-56945EF93500 ./target/debug/backendless-toolkit-db dump_table_data CorporateUser > CorporateUser.csv



./target/debug/backendless-toolkit-db dump_table_data ContactlessCardHistory > .var/database-jordan-dump-20261111/ContactlessCardHistory.csv



export BACKENDLESS_TOOLKIT__CONSOLE__APP_ID=7BB6D4A0-A9C6-4154-A6EE-ADE720D8D507

cat .var/database-jordan/CorporateAppModule.yaml | ./target/debug/backendless-toolkit-db restore_table_structure CorporateAppModule
cat CorporateAppModule.csv | ./target/debug/backendless-toolkit-db restore_table_data CorporateAppModule
cat .var/database-jordan/CorporateAppModule.yaml | ./target/debug/backendless-toolkit-db restore_table_structure CorporateAppModule --apply-required-flag --delete-obsolete-columns

cat .var/database-jordan/CorporateApprovalLevel.yaml | ./target/debug/backendless-toolkit-db restore_table_structure CorporateApprovalLevel
cat CorporateApprovalLevel.csv | ./target/debug/backendless-toolkit-db restore_table_data CorporateApprovalLevel
cat .var/database-jordan/CorporateApprovalLevel.yaml | ./target/debug/backendless-toolkit-db restore_table_structure CorporateApprovalLevel --apply-required-flag --delete-obsolete-columns

cat .var/database-jordan/CorporateApprovalPermission.yaml | ./target/debug/backendless-toolkit-db restore_table_structure CorporateApprovalPermission
cat CorporateApprovalPermission.csv | ./target/debug/backendless-toolkit-db restore_table_data CorporateApprovalPermission
cat .var/database-jordan/CorporateApprovalPermission.yaml | ./target/debug/backendless-toolkit-db restore_table_structure CorporateApprovalPermission --apply-required-flag --delete-obsolete-columns

cat .var/database-jordan/CorporateEmployee.yaml | ./target/debug/backendless-toolkit-db restore_table_structure CorporateEmployee
cat CorporateEmployee.csv | ./target/debug/backendless-toolkit-db restore_table_data CorporateEmployee
cat .var/database-jordan/CorporateEmployee.yaml | ./target/debug/backendless-toolkit-db restore_table_structure CorporateEmployee --apply-required-flag --delete-obsolete-columns

cat .var/database-jordan/CorporateUser.yaml | ./target/debug/backendless-toolkit-db restore_table_structure CorporateUser
cat CorporateUser.csv | ./target/debug/backendless-toolkit-db restore_table_data CorporateUser
cat .var/database-jordan/CorporateUser.yaml | ./target/debug/backendless-toolkit-db restore_table_structure CorporateUser --apply-required-flag --delete-obsolete-columns
```

### Test 2: Sync DB schema

```shell
BACKENDLESS_TOOLKIT__CONSOLE__APP_ID=BAF3E974-52AA-7598-FF04-56945EF93500 ./target/debug/backendless-toolkit-db show_tables > .var/database-jordan-dump-20261111.sync_list.txt

# Create a copy for each step to make able to modify it (exclude some parts independently)
cp -a .var/database-jordan-dump-20261111.sync_list.txt .var/database-jordan-dump-20261111.sync_list-shema.txt
cp -a .var/database-jordan-dump-20261111.sync_list.txt .var/database-jordan-dump-20261111.sync_list-data-dump.txt
cp -a .var/database-jordan-dump-20261111.sync_list.txt .var/database-jordan-dump-20261111.sync_list-data-restore.txt

# Sync DB schema to target instance
# Note: DeviceRegistration does not sync
(
  set -euo pipefail
  for T in $(cat .var/database-jordan-dump-20261111.sync_list-shema.txt); do
    echo "Sync schema for ${T}"
    cat ".var/database-jordan/${T}.yaml" \
      | BACKENDLESS_TOOLKIT__CONSOLE__APP_ID=7BB6D4A0-A9C6-4154-A6EE-ADE720D8D507 \
        ./target/debug/backendless-toolkit-db restore_table_structure "${T}"
  done
)

# Dump data from source instance
(
  set -euo pipefail
  for T in $(cat .var/database-jordan-dump-20261111.sync_list-data-dump.txt); do
    echo "Dump data from table ${T}"
    BACKENDLESS_TOOLKIT__CONSOLE__APP_ID=BAF3E974-52AA-7598-FF04-56945EF93500 \
        ./target/debug/backendless-toolkit-db dump_table_data "${T}" > ".var/database-jordan-dump-20261111/${T}.csv";
  done
)

# Restore data to target instance
(
  set -euo pipefail
  for T in $(cat .var/database-jordan-dump-20261111.sync_list-data-restore.txt); do
    echo "Restore data for table ${T}"
    cat ".var/database-jordan-dump-20261111/${T}.csv" \
      | BACKENDLESS_TOOLKIT__CONSOLE__APP_ID=7BB6D4A0-A9C6-4154-A6EE-ADE720D8D507 \
      ./target/debug/backendless-toolkit-db restore_table_data --bulk "${T}";
  done
)
```