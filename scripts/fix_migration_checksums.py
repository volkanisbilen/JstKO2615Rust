"""Fix _sqlx_migrations checksums after migration files were modified.

sqlx stores SHA-384 checksums of migration file contents in the _sqlx_migrations table.
When migration files are modified after being applied (e.g., adding ON CONFLICT DO NOTHING),
the checksums no longer match and sqlx refuses to run new migrations.

This script:
1. Reads all migration .sql files from disk
2. Computes their SHA-384 checksums
3. Compares with the checksums stored in _sqlx_migrations
4. Updates any mismatched checksums in the database

Usage:
    python scripts/fix_migration_checksums.py
"""
import hashlib
import os
import re
import sys

import psycopg2

PG_HOST = os.environ.get("PG_HOST", "localhost")
PG_PORT = int(os.environ.get("PG_PORT", "5432"))
PG_DB = os.environ.get("PG_DB", "ko_server")
PG_USER = os.environ.get("PG_USER", "koserver")
PG_PASS = os.environ.get("PG_PASS", "koserver123")

MIGRATIONS_DIR = os.environ.get(
    "MIGRATIONS_DIR",
    os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "migrations"),
)


def get_migration_version(filename: str) -> str | None:
    """Extract the version number from a migration filename like 20260207000001_create_account_char.sql."""
    match = re.match(r"^(\d+)_", filename)
    return match.group(1) if match else None


def compute_sha384(filepath: str) -> bytes:
    """Compute SHA-384 hash of file contents, returning raw bytes."""
    with open(filepath, "rb") as f:
        content = f.read()
    return hashlib.sha384(content).digest()


def main():
    # Discover all migration files
    if not os.path.isdir(MIGRATIONS_DIR):
        print(f"ERROR: Migrations directory not found: {MIGRATIONS_DIR}")
        sys.exit(1)

    migration_files = {}
    for fname in sorted(os.listdir(MIGRATIONS_DIR)):
        if not fname.endswith(".sql"):
            continue
        version = get_migration_version(fname)
        if version:
            migration_files[version] = os.path.join(MIGRATIONS_DIR, fname)

    print(f"Found {len(migration_files)} migration files on disk")

    # Connect to PostgreSQL
    conn = psycopg2.connect(
        host=PG_HOST, port=PG_PORT, dbname=PG_DB, user=PG_USER, password=PG_PASS
    )
    cur = conn.cursor()

    # Get all applied migrations from the database
    cur.execute("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
    db_migrations = {str(row[0]): bytes(row[1]) for row in cur.fetchall()}
    print(f"Found {len(db_migrations)} applied migrations in database")

    # Compare checksums
    mismatched = []
    missing_on_disk = []
    missing_in_db = []

    for version, db_checksum in db_migrations.items():
        if version not in migration_files:
            missing_on_disk.append(version)
            continue

        file_checksum = compute_sha384(migration_files[version])
        if file_checksum != db_checksum:
            mismatched.append((version, migration_files[version], file_checksum, db_checksum))

    for version in migration_files:
        if version not in db_migrations:
            missing_in_db.append(version)

    # Report
    if missing_on_disk:
        print(f"\nWARNING: {len(missing_on_disk)} migrations in DB but not on disk:")
        for v in missing_on_disk:
            print(f"  - {v}")

    if missing_in_db:
        print(f"\nINFO: {len(missing_in_db)} migration files not yet applied:")
        for v in sorted(missing_in_db):
            print(f"  - {v} ({os.path.basename(migration_files[v])})")

    if not mismatched:
        print("\nAll checksums match! Nothing to fix.")
        conn.close()
        return

    print(f"\nFound {len(mismatched)} mismatched checksums:")
    for version, filepath, file_hash, db_hash in mismatched:
        print(f"  - {version} ({os.path.basename(filepath)})")
        print(f"    DB:   {db_hash.hex()}")
        print(f"    File: {file_hash.hex()}")

    # Confirm before updating
    print(f"\nAbout to update {len(mismatched)} checksum(s) in _sqlx_migrations.")
    response = input("Proceed? [y/N] ").strip().lower()
    if response != "y":
        print("Aborted.")
        conn.close()
        return

    # Update checksums
    update_count = 0
    for version, filepath, file_hash, db_hash in mismatched:
        cur.execute(
            "UPDATE _sqlx_migrations SET checksum = %s WHERE version = %s",
            (psycopg2.Binary(file_hash), int(version))
        )
        if cur.rowcount == 1:
            update_count += 1
            print(f"  Updated: {version}")
        else:
            print(f"  FAILED to update: {version} (rowcount={cur.rowcount})")

    conn.commit()
    print(f"\nDone! Updated {update_count}/{len(mismatched)} checksums.")

    # Verify
    cur.execute("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
    db_after = {str(row[0]): bytes(row[1]) for row in cur.fetchall()}

    verify_ok = True
    for version, filepath, file_hash, _ in mismatched:
        if version in db_after and db_after[version] == file_hash:
            print(f"  Verified: {version} OK")
        else:
            print(f"  VERIFY FAILED: {version}")
            verify_ok = False

    if verify_ok:
        print("\nAll checksums verified successfully!")
        print("You can now run: sqlx migrate run")
    else:
        print("\nSome checksums failed verification! Please investigate.")

    conn.close()


if __name__ == "__main__":
    main()
