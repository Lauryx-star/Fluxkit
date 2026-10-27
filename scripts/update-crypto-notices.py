"""Refresh bundled upstream license files for the Vault cryptography dependency slice."""
import json
from pathlib import Path
import shutil
import subprocess

PACKAGES = (
    "aes-gcm", "argon2", "zeroize", "zeroize_derive", "aead", "ctr", "ghash",
    "polyval", "universal-hash", "opaque-debug", "blake2", "password-hash", "base64ct",
)

root = Path(__file__).resolve().parents[1]
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--manifest-path", str(root / "src-tauri/Cargo.toml"),
    "--locked", "--format-version", "1", "--offline",
], text=True))
destination = root / "assets/notices/crypto"
rows = []
for name in PACKAGES:
    packages = [p for p in metadata["packages"] if p["name"] == name]
    if len(packages) != 1:
        raise RuntimeError(f"Expected one locked package for {name}; review its notices manually.")
    package = packages[0]
    source = Path(package["manifest_path"]).parent
    target = destination / f"{name}-{package['version']}"
    target.mkdir(parents=True, exist_ok=True)
    notices = sorted(p for p in source.iterdir() if p.is_file()
                     and (p.name.startswith("LICENSE") or p.name in ("COPYRIGHT", "NOTICE")))
    if not notices:
        raise RuntimeError(f"No upstream license files found for {name}.")
    for notice in notices:
        shutil.copyfile(notice, target / notice.name)
    rows.append(f"| {name} | {package['version']} | {package['license']} |")
(destination / "README.md").write_text(
    "# Vault cryptography dependency notices\n\n"
    "Upstream license/copyright files copied unchanged from the packages locked in Cargo.lock.\n"
    "These files are included as desktop bundle resources.\n"
    "Regenerate with `python3 scripts/update-crypto-notices.py` after fetching locked dependencies.\n"
    "When a version is removed from the lockfile, review and remove its obsolete notice directory.\n\n"
    "| Package | Version | License expression |\n| --- | --- | --- |\n"
    + "\n".join(rows) + "\n", encoding="utf-8")
formatter = root / "node_modules/.bin/prettier"
subprocess.run([str(formatter), "--write", str(destination / "README.md")], check=True)
print(f"Updated notices for {len(PACKAGES)} locked Vault cryptography dependencies.")
