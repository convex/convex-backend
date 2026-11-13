#! /bin/bash

export DATA_DIR=${DATA_DIR:-/convex/data}
# Remember whether TMPDIR is ours (the default under DATA_DIR) before setting it.
if [ -z "${TMPDIR:-}" ]; then TMPDIR_IS_DEFAULT=1; else TMPDIR_IS_DEFAULT=0; fi
export TMPDIR=${TMPDIR:-"$DATA_DIR/tmp"}
export STORAGE_DIR=${STORAGE_DIR:-"$DATA_DIR/storage"}
export SQLITE_DB=${SQLITE_DB:-"$DATA_DIR/db.sqlite3"}

# Database driver flags matching DbDriverTag values
POSTGRES_DB_FLAGS=(--db postgres-v5)
MYSQL_DB_FLAGS=(--db mysql-v5)

set -e
mkdir -p "$TMPDIR" "$STORAGE_DIR"

# The default TMPDIR lives on the data volume, so temporary directories of a
# backend that was stopped or crashed (search archive caches, node executor
# packages) survive restarts and are never removed. Nothing uses it before the
# backend starts, so clear what earlier runs left behind. A TMPDIR set by the
# operator may be shared with other processes and is left alone, and so is a
# TMPDIR that holds persistent data (storage, SQLite or credentials).
# Set CLEAN_TMPDIR_ON_START=0 to keep them.
clean_tmpdir_on_start() {
    local tmp path
    if ! tmp=$(realpath -m -- "$TMPDIR"); then
        echo "WARNING: Cannot resolve $TMPDIR; skipping temporary-directory cleanup" >&2
        return 0
    fi
    for path in "$STORAGE_DIR" "$SQLITE_DB" "${CREDENTIALS_DIR:-$DATA_DIR/credentials}"; do
        if ! path=$(realpath -m -- "$path"); then
            echo "WARNING: Cannot resolve a persistent path; skipping temporary-directory cleanup" >&2
            return 0
        fi
        case "$path/" in
            "$tmp"/*)
                echo "Not clearing $TMPDIR: it contains $path"
                return 0
                ;;
        esac
    done
    if [ ! -r "$TMPDIR" ] || [ ! -x "$TMPDIR" ]; then
        echo "WARNING: Cannot list $TMPDIR; skipping temporary-directory cleanup" >&2
        return 0
    fi
    local start=$SECONDS count=0 failed=0
    for path in "$TMPDIR"/.tmp*; do
        # An unmatched glob is literal; include dangling symlinks as entries.
        if [ ! -e "$path" ] && [ ! -L "$path" ]; then continue; fi
        count=$((count + 1))
        if ! rm -rf -- "$path"; then failed=$((failed + 1)); fi
    done
    if [ "$failed" -gt 0 ]; then
        echo "WARNING: Failed to remove $failed of $count stale .tmp* entries from $TMPDIR; continuing startup" >&2
    fi
    echo "Cleared $((count - failed)) stale .tmp* entries from $TMPDIR in $((SECONDS - start))s"
}
if [ "${CLEAN_TMPDIR_ON_START:-1}" = "1" ] && [ "$TMPDIR_IS_DEFAULT" = "1" ]; then
    clean_tmpdir_on_start
fi

source ./read_credentials.sh

# Determine database configuration
if [ -n "$POSTGRES_URL" ]; then
    DB_SPEC="$POSTGRES_URL"
    DB_FLAGS=("${POSTGRES_DB_FLAGS[@]}")
elif [ -n "$MYSQL_URL" ]; then
    DB_SPEC="$MYSQL_URL"
    DB_FLAGS=("${MYSQL_DB_FLAGS[@]}")
elif [ -n "$DATABASE_URL" ]; then
    echo "Warning: DATABASE_URL is deprecated. Please use POSTGRES_URL for PostgreSQL or MYSQL_URL for MySQL connections instead."
    DB_SPEC="$DATABASE_URL"
    DB_FLAGS=("${POSTGRES_DB_FLAGS[@]}")  # Maintain backwards compatibility with existing DATABASE_URL behavior
else
    # Otherwise fallback to SQLite
    DB_SPEC="$SQLITE_DB"
    DB_FLAGS=()
fi

# Check if all required S3 environment variables are present
MISSING_VARS=()
[ -z "$AWS_REGION" ] && MISSING_VARS+=("AWS_REGION")
[ -z "$S3_STORAGE_EXPORTS_BUCKET" ] && MISSING_VARS+=("S3_STORAGE_EXPORTS_BUCKET")
[ -z "$S3_STORAGE_SNAPSHOT_IMPORTS_BUCKET" ] && MISSING_VARS+=("S3_STORAGE_SNAPSHOT_IMPORTS_BUCKET")
[ -z "$S3_STORAGE_MODULES_BUCKET" ] && MISSING_VARS+=("S3_STORAGE_MODULES_BUCKET")
[ -z "$S3_STORAGE_FILES_BUCKET" ] && MISSING_VARS+=("S3_STORAGE_FILES_BUCKET")
[ -z "$S3_STORAGE_SEARCH_BUCKET" ] && MISSING_VARS+=("S3_STORAGE_SEARCH_BUCKET")

if [ ${#MISSING_VARS[@]} -eq 0 ]; then
    STORAGE_FLAGS=(--s3-storage)
else
    if [ ${#MISSING_VARS[@]} -lt 6 ]; then
        echo "Warning: Some AWS/S3 environment variables are missing. Falling back to local storage."
        echo "Missing variables: ${MISSING_VARS[*]}"
    fi
    STORAGE_FLAGS=(--local-storage "$STORAGE_DIR")
fi

# --port and --site-proxy-port are internal to the container, so we pick them to
# avoid conflicts in the container.
# --convex-origin and --convex-site are how the backend can be contacted from
# the outside world. They show up in storage urls, action callbacks, etc.

exec ./convex-local-backend "$@" \
    --instance-name "$INSTANCE_NAME" \
    --instance-secret "$INSTANCE_SECRET" \
    --port 3210 \
    --site-proxy-port 3211 \
    --convex-origin "$CONVEX_CLOUD_ORIGIN" \
    --convex-site "$CONVEX_SITE_ORIGIN" \
    --beacon-tag "self-hosted-docker" \
    ${DISABLE_BEACON:+--disable-beacon} \
    ${REDACT_LOGS_TO_CLIENT:+--redact-logs-to-client} \
    ${DO_NOT_REQUIRE_SSL:+--do-not-require-ssl} \
    "${DB_FLAGS[@]}" \
    "${STORAGE_FLAGS[@]}" \
    "$DB_SPEC"
