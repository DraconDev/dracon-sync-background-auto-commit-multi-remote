#!/usr/bin/env python3
"""Regression tests for the read-only initial fleet inventory (no cloud access)."""
import importlib.util
import json
import pathlib
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    'bucket_inventory', pathlib.Path(__file__).with_name('inventory-bucket-discovery.py'))
INVENTORY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INVENTORY)


class InventoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='bucket-inventory-test-')
        self.addCleanup(self.temp.cleanup)
        self.repo = pathlib.Path(self.temp.name) / 'project'
        self.repo.mkdir()
        subprocess.run(['git', 'init', '--quiet', str(self.repo)], check=True,
                       capture_output=True, timeout=10)

    def write(self, path, contents='not eligibility evidence'):
        dest = self.repo / path
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(contents)
        return dest

    def test_new_and_tracked_media_are_candidates_not_eligibility(self):
        self.write('static/generated.png')
        self.write('public/new.webp')
        self.write('src/source.ts')
        subprocess.run(['git', '-C', str(self.repo), 'add', '--', 'static/generated.png'],
                       check=True, capture_output=True, timeout=10)
        result = INVENTORY.inspect(self.repo)
        self.assertEqual(result['mediaPaths'], 2)
        self.assertEqual(result['enrollment'], 'not_qualified')
        self.assertEqual({r['path'] for r in result['candidateAssetRoots']}, {'public', 'static'})
        self.assertIn('no verified generator', result['reason'])
        self.assertEqual((self.repo / 'static/generated.png').read_text(), 'not eligibility evidence')

    def test_excluded_and_ignored_paths_do_not_become_candidates(self):
        self.write('.gitignore', 'scratch/\n')
        for name in ('secrets/output.png', 'node_modules/large.png',
                     'target/media.png', '.env.private/image.png', 'scratch/ignored.webp'):
            self.write(name)
        self.assertEqual(INVENTORY.inspect(self.repo)['mediaPaths'], 0)

    def test_config_does_not_grant_discovery_eligibility(self):
        self.write('static/icon.png')
        self.write('bucket-assets.json', json.dumps({
            'enabled': True, 'assetRoot': 'static', 'manifest': 'manifest.json',
            'uploaderCredentials': '/credentials/must/not/be/read'}))
        result = INVENTORY.inspect(self.repo)
        self.assertTrue(result['configuration']['enabled'])
        self.assertEqual(result['enrollment'], 'not_qualified')
        self.assertNotIn('uploaderCredentials', result['configuration'])

    def test_symlink_configuration_is_refused_without_reading_target(self):
        target = pathlib.Path(self.temp.name) / 'outside.json'
        target.write_text('this is intentionally not parsed')
        (self.repo / 'bucket-assets.json').symlink_to(target)
        self.assertEqual(INVENTORY.config_summary(self.repo)['error'], 'symlink path refused')

    def test_configuration_below_symlink_parent_is_refused(self):
        self.write('bucket-assets.json', '{"enabled": true}')
        alias = pathlib.Path(self.temp.name) / 'alias'
        alias.symlink_to(self.repo, target_is_directory=True)
        self.assertEqual(INVENTORY.config_summary(alias)['error'], 'symlink path refused')

    def test_missing_repository_is_explicit_not_empty_success(self):
        result = INVENTORY.inspect(self.repo / 'missing')
        self.assertIn('enumeration failed', result['reason'])
        self.assertEqual(result['enrollment'], 'not_qualified')


if __name__ == '__main__':
    unittest.main()
