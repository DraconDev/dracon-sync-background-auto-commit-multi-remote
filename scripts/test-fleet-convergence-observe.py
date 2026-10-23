#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import sys
import tempfile
import time
import unittest

spec = importlib.util.spec_from_file_location('observer', Path(__file__).with_name('fleet-convergence-observe.py'))
observer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(observer)


class ObservationTests(unittest.TestCase):
    def test_primary_origin_is_required_even_without_mirror_targets(self):
        self.assertEqual(observer.required_remotes({'push_to_remotes': []}), ['origin'])
        self.assertEqual(observer.required_remotes({'push_to_remotes': ['origin', 'gitlab']}), ['origin', 'gitlab'])

    def test_ref_errors_missing_refs_and_malformed_output_are_not_tips(self):
        for width in (40, 64):
            oid = 'a' * width
            self.assertEqual(observer.advertised_tip(0, f'{oid}\trefs/heads/main\n', 'main'), oid)
        self.assertIsNone(observer.advertised_tip(1, 'a' * 40 + '\trefs/heads/main', 'main'))
        self.assertIsNone(observer.advertised_tip(0, '', 'main'))
        for output in ['-flag refs/heads/main', 'a' * 40 + ' refs/heads/wrong', ('a' * 40 + ' refs/heads/main\n') * 2]:
            with self.assertRaises(RuntimeError):
                observer.advertised_tip(0, output, 'main')

    def test_url_receipts_do_not_copy_credentials_or_query_tokens(self):
        self.assertEqual(observer.safe_url('https://user:password@example.test/repo.git?token=value#fragment'), 'https://example.test/repo.git')
        self.assertEqual(observer.safe_url('git@example.test:repo.git'), 'example.test:repo.git')

    def test_timeout_cleans_owned_descendant_and_inherited_pipes(self):
        with tempfile.TemporaryDirectory() as root:
            marker = Path(root) / 'child-pid'
            script = ('import subprocess,sys,time;from pathlib import Path;'
                      'child=subprocess.Popen([sys.executable,"-c","import time;time.sleep(30)"]);'
                      f'Path({str(marker)!r}).write_text(str(child.pid));time.sleep(30)')
            started = time.monotonic()
            code, _, _ = observer.run([sys.executable, '-c', script], timeout=1)
            self.assertEqual(code, 124)
            self.assertTrue(marker.exists(), 'the owned descendant must actually start')
            self.assertLess(time.monotonic() - started, 5)
            stat = Path('/proc') / marker.read_text() / 'stat'
            if stat.exists():
                self.assertEqual(stat.read_text().rsplit(') ', 1)[1].split()[0], 'Z')


if __name__ == '__main__':
    unittest.main()
