#!/usr/bin/env python3
"""Create signed, encrypted SUFE bootstrap objects. Requires cryptography.

The Ed25519 private key stays on the operator's computer/CI secret store.
The application contains only its public key and the separate OSS password.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import secrets
import time

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.hkdf import HKDF


def b64(value: bytes) -> str:
    return base64.b64encode(value).decode("ascii")


def pack(document: dict, password: str, signing_key: Ed25519PrivateKey, nonce=None) -> dict:
    if not password:
        raise ValueError("SUFE_BOOTSTRAP_PASSWORD is empty")
    if not document.get("project_id") or not document.get("api_endpoints"):
        raise ValueError("project_id and api_endpoints are required")
    nonce = nonce if nonce is not None else secrets.token_bytes(12)
    key = HKDF(algorithm=hashes.SHA256(), length=64, salt=b"sufe-bootstrap-v1",
               info=b"sufe-bootstrap-config/v1").derive(password.encode())[:32]
    plain = json.dumps(document, separators=(",", ":"), ensure_ascii=False).encode()
    encrypted = AESGCM(key).encrypt(nonce, plain, document["project_id"].encode())
    envelope = {"version": 1, "nonce": b64(nonce), "ciphertext": b64(encrypted)}
    signed = f"sufe-bootstrap-v1\n{envelope['nonce']}\n{envelope['ciphertext']}".encode()
    envelope["signature"] = b64(signing_key.sign(signed))
    return envelope


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    generate = sub.add_parser("keygen", help="Create a private key; prints only its public key")
    generate.add_argument("private_key", type=Path)
    encrypt = sub.add_parser("pack", help="Encrypt and sign an operator deployment document")
    encrypt.add_argument("document", type=Path)
    encrypt.add_argument("private_key", type=Path)
    encrypt.add_argument("output", type=Path)
    encrypt.add_argument("--valid-days", type=int, default=7)
    args = parser.parse_args()
    if args.action == "keygen":
        key = Ed25519PrivateKey.generate()
        # Exclusive creation avoids overwriting an existing deployment's trust root.
        with args.private_key.open("xb") as output:
            output.write(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8,
                                           serialization.NoEncryption()))
        if os.name != "nt":
            args.private_key.chmod(0o600)
        print(b64(key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)))
        return
    if not 1 <= args.valid_days <= 90:
        parser.error("--valid-days must be between 1 and 90")
    password = os.environ.get("SUFE_BOOTSTRAP_PASSWORD", "")
    if not password:
        parser.error("Set SUFE_BOOTSTRAP_PASSWORD; passwords are never accepted as command arguments")
    key = serialization.load_pem_private_key(args.private_key.read_bytes(), password=None)
    if not isinstance(key, Ed25519PrivateKey):
        parser.error("The private key must be Ed25519")
    document = json.loads(args.document.read_text(encoding="utf-8-sig"))
    document["issued_at"] = int(time.time())
    document["expires_at"] = document["issued_at"] + args.valid_days * 86400
    payload = pack(document, password, key)
    args.output.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    print(f"Encrypted configuration written to {args.output}")


if __name__ == "__main__":
    main()
