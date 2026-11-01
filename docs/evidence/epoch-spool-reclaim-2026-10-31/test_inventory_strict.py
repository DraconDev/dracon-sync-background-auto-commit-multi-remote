"""Disposable-fixture tests; no live repositories or reaper calls."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('inventory_strict', Path(__file__).with_name('inventory-strict.py'))
inv = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inv)


class InventoryTests(unittest.TestCase):
    def test_hardlinks_are_counted_logically_but_allocation_once(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d)
            (p / 'a').write_bytes(b'recovery')
            os.link(p / 'a', p / 'b')
            result, inodes = inv.measure(p)
            self.assertEqual(result['logical_bytes'], 16)
            self.assertEqual(result['errors'], [])
            self.assertEqual(len(inodes), 2)  # directory plus one file inode
            self.assertEqual(sum(inodes.values()), p.stat().st_blocks * 512 + (p / 'a').stat().st_blocks * 512)

    def test_read_error_and_symlink_are_not_silent_zeroes(self):
        with tempfile.TemporaryDirectory() as d:
            with patch.object(inv.os, 'scandir', side_effect=PermissionError('fixture denial')):
                result, _ = inv.measure(Path(d))
            self.assertIsNone(result['logical_bytes'])
            self.assertTrue(result['errors'])
            (Path(d) / 'outside').symlink_to('/does-not-exist')
            result, _ = inv.measure(Path(d))
            self.assertIsNone(result['logical_bytes'])
            self.assertIn('symlink not followed', str(result['errors']))

    def test_marker_type_identity_and_legacy_classification(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d)
            self.assertEqual(inv.ownership(p, 0)[0], 'ownership_uncertain_legacy')
            (p / 'original-index').write_bytes(b'preserve')
            self.assertEqual(inv.ownership(p, 0)[0], 'preserve_marked')
            (p / 'original-index').unlink()  # only disposable fixture input
            lock = p / 'reclaim-owner.lock'
            lock.write_bytes(b'')
            ls, ds = lock.stat(), p.stat()
            identity = dict(version=1, directory_device=ds.st_dev, directory_inode=ds.st_ino,
                            lock_device=ls.st_dev, lock_inode=ls.st_ino)
            marker = p / 'reclaim-settled.json'
            marker.write_text(json.dumps(identity))
            self.assertEqual(inv.ownership(p, 0)[0], 'ownership_eligible_candidate')
            with lock.open('rb') as held:
                inv.fcntl.flock(held, inv.fcntl.LOCK_EX | inv.fcntl.LOCK_NB)
                self.assertEqual(inv.ownership(p, 0)[0], 'active_owner')
            identity['version'] = True  # Python equality must not admit Rust-invalid JSON types
            marker.write_text(json.dumps(identity))
            self.assertEqual(inv.ownership(p, 0)[0], 'ownership_uncertain_protocol')

    def test_git_worktree_common_and_nested_module_discovery(self):
        with tempfile.TemporaryDirectory() as d:
            root, checkout = Path(d), Path(d) / 'repo'
            env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
            env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null')
            def git(*args):
                return subprocess.run(['git', '-c', 'core.hooksPath=/dev/null',
                                       '-c', 'user.name=DraconDev', '-c', 'user.email=dracsharp@gmail.com',
                                       *map(str, args)], env=env, check=True, capture_output=True)
            git('init', '--template=', checkout)
            git('-C', checkout, 'commit', '--allow-empty', '-m', 'disposable inventory fixture')
            worktree = root / 'linked'
            git('-C', checkout, 'worktree', 'add', '--detach', worktree)
            metadata = checkout / '.git'
            nested = metadata / 'modules' / 'deep' / 'module'
            nested.mkdir(parents=True)
            git('init', '--bare', '--template=', nested)
            for parent in (metadata, metadata / 'worktrees' / 'linked', nested):
                spool = parent / 'dracon-staging-fixture'
                spool.mkdir()
                (spool / 'index').write_bytes(b'index')
            report = inv.inventory({'repos': [str(worktree)]}, 0)
            self.assertTrue(report['complete'], report)
            self.assertEqual(report['spool_count'], 3)
            self.assertEqual(report['spool_logical_bytes'], 15)
            self.assertEqual(report['categories'], {'ownership_uncertain_legacy': 3})
            self.assertFalse(report['deletion_authority'])


if __name__ == '__main__':
    unittest.main()
