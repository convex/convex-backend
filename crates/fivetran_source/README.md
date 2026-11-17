# Fivetran Source Connector

This crate contains a source connector allowing developers using Convex to
replicate the data they store in Convex to other databases.

The connector consists of a gRPC server hosted on the Fivetran infrastructure.
This server retrieves the data it needs using the HTTP API described
[in the Convex docs](https://docs.convex.dev/http-api/).

## Installation

Make sure you have Git and Cargo installed. We recommend installing Cargo via
[rustup](https://rustup.rs/).

```
git clone https://github.com/convex/convex-backend.git
cd convex-backend
cargo build --release -p fivetran_source
```

You can then find the executable file in
`convex-backend/target/release/fivetran_source`.

## Usage

You can start the connector by starting its binary:

```
$ ./fivetran_source
Starting the connector on [::]:50051
```

You can change the port used using the optional `--port` parameter:

```
$ ./fivetran_source --port 1337
Starting the connector on [::]:1337
```

## Local testing

Fivetran publishes a
[connector tester](https://github.com/fivetran/fivetran_partner_sdk/blob/main/tools/source-connector-tester/README.md)
that drives the connector the way Fivetran itself does — running the setup
tests, fetching the schema, streaming `Update`, and applying the records to a
local DuckDB warehouse. Point it at a connector running against
`just run-local-backend`:

```sh
# 1. Backend, with an admin key matching its instance secret.
just run-local-backend
ADMIN_KEY=$(cargo run --quiet --bin generate_key -- \
  "$(cat crates/keybroker/dev/instance_name.txt)" \
  "$(cat convex_local_storage/dev_instance_secret)")

# 2. Some data to sync, imported from any directory with a package.json
#    depending on `convex`.
npx convex import --admin-key "$ADMIN_KEY" --url http://127.0.0.1:8000 \
  --table teams --format jsonLines teams.jsonl

# 3. The connector.
cargo run --bin fivetran_source -- --port 50051

# 4. The tester. It prompts for the configuration form fields on the first run
#    and persists them, so later runs are incremental syncs.
mkdir -p /tmp/ft_tester_data
docker run --mount type=bind,source=/tmp/ft_tester_data,target=/data \
  -a STDIN -a STDOUT -a STDERR -it \
  -e GRPC_HOSTNAME=host.docker.internal --network=host \
  us-docker.pkg.dev/build-286712/public-docker-us/sdktesters-v2/sdk-tester:<version> \
  --tester-type source --port 50051
```

Answer the prompts with `http://127.0.0.1:8000` and the admin key — the
connector runs on the host, so it reaches the backend over loopback rather than
`host.docker.internal`.

Inspect the result with `duckdb /tmp/ft_tester_data/warehouse.db`. Editing the
tester's `/data` directory between runs exercises the interesting paths — in
`schema_selection.txt`, flip a table's `[x]` to `[ ]` and re-run to check that
the deselected table keeps its synced data but stops updating, then back to
`[x]` to check that reselecting it truncates and backfills the table.

## Sync Mechanism

The connector is powered by the Convex data sync API
([`POST /api/v1/data/sync`](https://docs.convex.dev/deployment-api/data-sync)),
a single paginated stream covering both the initial copy of a deployment's data
and the changes that follow. Its docs describe the pagination, consistency, and
status semantics.

Each page carries an opaque cursor, which the connector checkpoints with
Fivetran so the next sync resumes where this one stopped. It keeps fetching
pages until the API reports the sync has caught up to the latest data, then
waits for Fivetran to schedule the next sync.

The API also reports which tables the destination must drop and refill — a
table's first sync, one added to the selection, or one replaced wholesale (for
instance by `npx convex import`) — which the connector passes on to Fivetran as
truncates. A table removed from the selection is not truncated: like any
Fivetran connector, its synced data stays in the destination and stops updating
until the table is selected again.
