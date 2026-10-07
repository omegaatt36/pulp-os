"""Build/verify a real alternate-font fixture; never distribute its output."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'scripts/build-cjk-fonts.py'


class FontBundleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        assert SCRIPT.exists(), 'single-font manifest builder is missing'
        spec = importlib.util.spec_from_file_location('font_builder', SCRIPT)
        cls.builder = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.builder)
        cls.converter = cls.builder.converter_binary()

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        font = ROOT / 'assets/fonts/Bookerly-Regular.ttf'
        self.license = self.base / 'license.txt'
        self.license.write_text('Test fixture only; no redistribution.\n')
        self.manifest = self.base / 'font.json'
        self.data = dict(schema_version=1, name='alternate-fixture', version='fixture',
                         upstream_url='https://example.invalid/fixture', sizes=[16],
                         license_name='Test fixture permission',
                         font=dict(path=str(font), sha256=self.hash(font)),
                         license=dict(path='license.txt', sha256=self.hash(self.license)))
        self.write_manifest()
        self.out = self.base / 'sd'
        self.cache = self.base / 'cache'

    def hash(self, path):
        return hashlib.sha256(path.read_bytes()).hexdigest()

    def write_manifest(self):
        self.manifest.write_text(json.dumps(self.data))

    def build(self):
        return self.builder.build(self.manifest, self.out, self.cache, self.converter)

    def test_single_font_bundle_has_exact_license_and_verified_cache_hit(self):
        self.assertFalse(self.build())
        fonts = self.out / '_PULP/FONTS'
        self.assertTrue((fonts / 'F00016.PFN').is_file())
        self.assertEqual((fonts / 'LICENSE.TXT').read_bytes(), self.license.read_bytes())
        self.assertTrue((fonts / 'COVERAGE.TXT').is_file())
        self.assertIn('license_name=Test fixture permission', (fonts / 'PROV.TXT').read_text())
        self.assertTrue(self.build())
        (fonts / 'STALE.PFN').write_bytes(b'stale')
        self.assertTrue(self.build())
        self.assertFalse((fonts / 'STALE.PFN').exists())

    def test_corrupt_cached_pack_is_rebuilt(self):
        self.build()
        pack = next(self.cache.glob('*/F00016.PFN'))
        original = pack.read_bytes()
        pack.write_bytes(b'corrupt')
        self.assertFalse(self.build())
        self.assertEqual(pack.read_bytes(), original)

    def test_changed_verified_license_invalidates_cache(self):
        self.build()
        self.license.write_text('Changed fixture permission.\n')
        self.data['license']['sha256'] = self.hash(self.license)
        self.write_manifest()
        self.assertFalse(self.build())

    def test_source_hash_mismatch_is_rejected_before_output(self):
        self.data['font']['sha256'] = '0' * 64
        self.write_manifest()
        with self.assertRaisesRegex(ValueError, 'SHA256 mismatch'):
            self.build()
        self.assertFalse(self.out.exists())

    def test_unowned_font_directory_is_preserved_even_with_unrelated_marker(self):
        fonts = self.out / '_PULP/FONTS'
        fonts.mkdir(parents=True)
        (fonts / 'BUNDLE.JSON').write_text('{}')
        (fonts / 'important.txt').write_text('preserve')
        with self.assertRaisesRegex(ValueError, 'unowned'):
            self.build()
        self.assertEqual((fonts / 'important.txt').read_text(), 'preserve')

    def test_missing_required_characters_do_not_publish_a_bundle(self):
        import subprocess
        required = self.base / 'required.txt'
        required.write_text('\U0002a6a5')
        self.data['require_chars'] = dict(path='required.txt', sha256=self.hash(required))
        self.write_manifest()
        with self.assertRaises(subprocess.CalledProcessError) as failure:
            self.build()
        self.assertEqual(failure.exception.returncode, 3)
        self.assertFalse(self.out.exists())

    def test_cache_artifact_symlink_is_rejected_without_touching_target(self):
        import shutil
        self.build()
        artifact = next(self.cache.iterdir())
        victim = self.base / 'victim'
        victim.mkdir()
        (victim / 'important.txt').write_text('preserve')
        shutil.rmtree(artifact)
        artifact.symlink_to(victim, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'symlink'):
            self.build()
        self.assertEqual((victim / 'important.txt').read_text(), 'preserve')

    def test_sd_parent_symlink_is_rejected_without_touching_target(self):
        victim = self.base / 'victim'
        victim.mkdir()
        self.out.mkdir()
        (self.out / '_PULP').symlink_to(victim, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'symlink'):
            self.build()
        self.assertEqual(list(victim.iterdir()), [])

    def test_duplicate_or_invalid_sizes_are_rejected(self):
        for sizes in ([16, 16], [], [0], [256], [True]):
            self.data['sizes'] = sizes
            self.write_manifest()
            with self.assertRaisesRegex(ValueError, 'sizes'):
                self.build()


if __name__ == '__main__':
    unittest.main()
