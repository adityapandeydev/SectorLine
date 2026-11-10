import unittest
from pathlib import Path
import tempfile
import tarfile
import zipfile
import io
import package_release as release
class PackageTests(unittest.TestCase):
    def test_archives_reject_extra_files_duplicates_and_links(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "test.zip"
            with zipfile.ZipFile(archive, "w") as dest:
                dest.writestr("app/bin", b"binary")
            self.assertEqual(release.archive_members(archive, {"app/bin"}),
                             {"app/bin": b"binary"})
            with zipfile.ZipFile(archive, "a") as dest:
                dest.writestr("../escape", b"unexpected")
            with self.assertRaises(ValueError):
                release.archive_members(archive, {"app/bin"})
            archive = Path(directory) / "test.tar.gz"
            with tarfile.open(archive, "w:gz") as dest:
                member = tarfile.TarInfo("app/bin")
                member.size = 6
                dest.addfile(member, io.BytesIO(b"binary"))
            self.assertEqual(release.archive_members(archive, {"app/bin"}),
                             {"app/bin": b"binary"})
            with tarfile.open(archive, "w:gz") as dest:
                member = tarfile.TarInfo("app/bin")
                member.type = tarfile.SYMTYPE
                member.linkname = "/etc/passwd"
                dest.addfile(member)
            with self.assertRaises(ValueError):
                release.archive_members(archive, {"app/bin"})

    def test_known_targets_have_explicit_support_levels(self):
        self.assertEqual(release.target_info("x86_64-pc-windows-msvc"), ("sectorline.exe", False))
        self.assertEqual(release.target_info("x86_64-unknown-linux-gnu"), ("sectorline", True))
        self.assertEqual(release.target_info("aarch64-apple-darwin"), ("sectorline", True))
        for target in ("../../escape", "x86_64-apple-darwin", "latest"):
            with self.assertRaises(ValueError): release.target_info(target)
    def test_tag_must_match_package_version(self):
        release.check_tag("v0.1.0", "0.1.0")
        release.check_tag("", "0.1.0")  # Manual build creates artifacts, not a tag.
        for tag in ("v0.2.0", "0.1.0", "v0.1.0;echo bad"):
            with self.assertRaises(ValueError): release.check_tag(tag, "0.1.0")
if __name__ == "__main__": unittest.main()
