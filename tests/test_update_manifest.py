import base64
import importlib.util
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("update_manifest", Path(__file__).parents[1] / "scripts/update_manifest.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class UpdateManifestTests(unittest.TestCase):
    def test_uses_versioned_public_asset_url_and_signature_content(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "DeskBuddy.app.tar.gz"
            archive.write_bytes(b"archive")
            signature = Path(str(archive) + ".sig")
            encoded = base64.b64encode(b"untrusted comment: signature\npublic signature").decode()
            signature.write_text(encoded)
            result = module.make_manifest("0.6.0", "owner/deskbuddy", archive, signature, "Release notes")
            platform = result["platforms"]["darwin-aarch64"]
            self.assertEqual(platform["signature"], encoded)
            self.assertEqual(platform["url"], "https://github.com/owner/deskbuddy/releases/download/v0.6.0/DeskBuddy.app.tar.gz")
            self.assertEqual(result["notes"], "Release notes")
            with self.assertRaises(ValueError):
                module.make_manifest("0.6.0", "../wrong", archive, signature, "")
            signature.write_text("not a signature")
            with self.assertRaises(ValueError):
                module.make_manifest("0.6.0", "owner/deskbuddy", archive, signature, "")

    def test_rejects_missing_archive_and_unstable_versions(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "DeskBuddy.app.tar.gz"
            signature = Path(str(archive) + ".sig")
            with self.assertRaises(ValueError):
                module.make_manifest("0.6.0", "owner/deskbuddy", archive, signature, "")
            with self.assertRaises(ValueError):
                module.make_manifest("latest", "owner/deskbuddy", archive, signature, "")
