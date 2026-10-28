---
name: Convex
title: Convex connector for Fivetran
description: Documentation and setup guide for the Convex connector for Fivetran
---

# Convex {% badge text="Partner-Built" /%} {% availabilityBadge connector="convex" /%}

[Convex](https://convex.dev) is a full-stack TypeScript development platform. Replace your database, server functions, and glue code.

{% note %}
This connector is [partner-built](/docs/partner-built-program). For any questions related to the Convex connector and its documentation, refer to Convex's support team. For details on SLA, see [Convex's Status and Guarantees documentation](https://docs.convex.dev/production/state).
{% /note %}

----

## Features

{% featureTable connector="convex" /%}

-----

{% partial file="saas-supported-deployment-models.template.md" /%}

-----

## Setup guide

Follow the [step-by-step Convex setup guide](/docs/connectors/databases/convex/setup-guide) to connect your Convex database with Fivetran.

---

## Sync overview

Once Fivetran is connected to your Convex deployment, the connection fetches an initial consistent snapshot of all data from your Convex database. Once the initial sync is complete, the connection uses Change data capture (CDC) to efficiently incrementally sync updates at a newer consistent view of your Convex deployment. You can configure the frequency of these updates.

Each sync resumes from where the previous one stopped, so an interrupted sync never restarts from the beginning.

---

## Configuration

To configure a Convex connection, you need your deployment URL and deploy key. You can find both on your project's [Production Deployment Settings page](https://docs.convex.dev/dashboard/deployments/deployment-settings).

---

## Schema information

Fivetran tries to replicate the database and columns from your configured Convex deployment to your destination according to Fivetran's [standard database update strategies](/docs/connectors/databases#transformationandmappingoverview).

### Type transformations and mapping

As the connection extracts your data, it matches [Convex data types](https://docs.convex.dev/database/types) to types that Fivetran supports.

The following table illustrates how the connection transforms your Convex data types into Fivetran-supported types:

| Convex Type | Fivetran Type | Fivetran Supported |
| ----------- | ------------- | ------------------ |
| Id          | STRING        | True               |
| Null        | NULL          | True               |
| Int64       | LONG          | True               |
| Float64     | DOUBLE        | True               |
| Boolean     | BOOLEAN       | True               |
| String      | STRING        | True               |
| Bytes       | BINARY        | True               |
| Array       | JSON          | True               |
| Object      | JSON          | True               |

{% note %}
The `_creationTime` system field  in each document is special-cased to convert into a UTC_DATETIME, despite being stored as a Float64 inside of Convex.
{% /note %}

{% note %}
Nested types inside Object and Array are serialized as JSON using the [JSON format for export](https://docs.convex.dev/database/types).
{% /note %}

### Nested data

Convex documents are represented as JSON [by using conversions](https://docs.convex.dev/database/types). If the first-level field is a simple data type, the connection will map it to its own type. If it's a complex nested data type such as an array or JSON data, it maps to a JSON type without unpacking. The connection does not automatically unpack nested JSON objects to separate tables in the destination. Any nested JSON objects are preserved as is in the destination so that you can use JSON processing functions.

For example, the following Convex document:

```json
{"street"  : "Main St."
"city"     : "New York"
"country"  : "US"
"phone"    : "(555) 123-5555"
"zip code" : 12345
"people"   : ["John", "Jane", "Adam"]
"car"      : {"make" : "Honda",
              "year" : 2014,
              "type" : "AWD"}
}
```

is converted to the following table when the connection loads it into your destination:

| \_id | street   | city     | country | phone          | zip code | people                   | car                                               |
| ---- | -------- | -------- | ------- | -------------- | -------- | ------------------------ | ------------------------------------------------- |
| 1    | Main St. | New York | US      | (555) 123-5555 | 12345    | ["John", "Jane", "Adam"] | {"make" : "Honda", "year" : 2014, "type" : "AWD"} |

### Fivetran-generated data

Fivetran adds the following column to every table in your destination:

- `_fivetran_synced` (UTC TIMESTAMP) indicates the time when Fivetran last successfully synced the row. It is added to every table.
- `_fivetran_deleted` (BOOLEAN) indicates if the column was deleted in the source.

Fivetran adds these columns to give you insight into the state of your data and the progress of your data syncs. For more information about these columns, see [our System Columns and Tables documentation](/docs/core-concepts/system-columns-and-tables).

### Excluding source data

If you don’t want to sync all the data from your Convex database, you can exclude Convex components, tables, and fields from your syncs on your Fivetran dashboard. To do so, go to your Convex connection details page and uncheck the objects you would like to omit from syncing. 

For more information, see our [Data Blocking documentation](/docs/using-fivetran/features/data-blocking-column-hashing).

{% note %}
Newly included components and tables are backfilled from your Convex deployment on the next sync, with no historical re-sync needed. When you exclude a component or table, its synced data stays in your destination and stops updating. Selecting it again truncates and backfills the affected tables from your Convex deployment.
{% /note %}

{% important %}
Changing which *columns* are included only affects rows synced from that point on; rows already in your destination are not rewritten. To apply a column change to them, [trigger a historical re-sync](/docs/connectors/troubleshooting/trigger-historical-re-syncs).
{% /important %}
