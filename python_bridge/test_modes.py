"""Offline checks that demo generation cannot fall back to fake live ingestion."""
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import extract


class ModeTests(unittest.TestCase):
    def test_live_requests_fail_before_creating_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "not-created"
            for gp, live in [("live", False), ("LIVE", False), ("Monza", True)]:
                with self.assertRaisesRegex(RuntimeError, "Live access is unavailable"):
                    extract.export_f1_cache(str(output), gp=gp, live_mode=live)
                self.assertFalse(output.exists())

    def test_metadata_failure_does_not_call_live_fallback(self):
        with tempfile.TemporaryDirectory() as temporary:
            with patch.object(extract, "extract_circuit_with_fastf1", return_value=None):
                with patch.object(extract, "extract_live_from_openf1") as live:
                    with self.assertRaisesRegex(RuntimeError, "metadata extraction failed"):
                        extract.export_f1_cache(temporary, year=2024, gp="Monza")
                    live.assert_not_called()
            self.assertFalse((Path(temporary) / "telemetry_stream.json").exists())

    def test_direct_live_helper_is_disabled(self):
        with self.assertRaisesRegex(RuntimeError, "Live access is unavailable"):
            extract.extract_live_from_openf1("unused")


if __name__ == "__main__":
    unittest.main()
