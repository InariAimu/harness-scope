"""Create an isolated, synthetic demo database; never reads a real harness directory."""
import argparse
import json
import sqlite3
import uuid
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--db", default="output/playwright/demo.db")
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
database = Path(args.db).resolve()
if database.exists():
    raise SystemExit(f"Refusing to replace an existing database: {database}")
database.parent.mkdir(parents=True, exist_ok=True)
config = {
    "deviceName": "Demo fixtures",
    "sources": [
        {"id": h, "harness": h, "root": str(root / "tests" / "fixtures" / h), "enabled": True}
        for h in ("claude", "codex")
    ],
    "serverUrl": "", "serverToken": "", "scanIntervalSeconds": 60,
    "theme": "light", "language": "zh", "timezone": "Asia/Shanghai",
    "modelsDevSync": {"autoSyncEnabled": False, "includeCommonModels": True,
                      "selectedModelKeys": [], "excludedCommonModelKeys": [],
                      "lastSyncAt": None, "lastSyncError": None},
}
with sqlite3.connect(database) as db:
    db.executescript((root / "crates/core/src/schema.sql").read_text(encoding="utf-8"))
    db.executemany("INSERT INTO meta(key,value) VALUES(?,?)", [
        ("config", json.dumps(config)),
        ("device_id", json.dumps(str(uuid.uuid4()))),
        ("server_instance_id", json.dumps(str(uuid.uuid4()))),
    ])
print(database)
