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

    def test_intended_destinations_resolve_transport_aliases_but_not_ambiguity(self):
        remotes = {
            'origin': {'url_without_credentials': 'https://github.com/DraconDev/project.git'},
            'gitlab': {'url_without_credentials': 'gitlab.com:DraconDev/project.git'},
        }
        mapped = observer.intended_destinations(['origin', 'github', 'gitlab'], remotes)
        self.assertEqual(mapped[0]['destination'], mapped[1]['destination'])
        self.assertEqual(mapped[1]['aliases'], ['origin'])
        # Adding an equivalent transport alias is harmless, but a distinct
        # GitHub destination must not be guessed to satisfy a missing mirror.
        remotes['backup'] = {'url_without_credentials': 'github.com:dracondev/PROJECT.git'}
        self.assertEqual(observer.intended_destinations(['github'], remotes)[0]['aliases'], ['origin', 'backup'])
        remotes['foreign'] = {'url_without_credentials': 'github.com:another/project.git'}
        self.assertEqual(observer.intended_destinations(['github'], remotes)[0]['resolution'], 'unresolved')
        self.assertEqual(observer.intended_destinations(['missing'], remotes)[0]['resolution'], 'unresolved')
        self.assertEqual(observer.intended_destinations(['codeberg'], remotes)[0]['resolution'], 'unresolved')

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
            stat = Path('/proc') / marker.read_text() / 'stat'
            # SIGKILL delivery is asynchronous: the parent can be reaped and
            # pipes closed before the child has been scheduled to exit. Keep
            # the original five-second cleanup budget, not an instantaneous
            # process-state assertion that races a healthy kernel.
            state = None
            while time.monotonic() - started < 5:
                try:
                    state = stat.read_text().rsplit(') ', 1)[1].split()[0]
                except FileNotFoundError:
                    state = None
                if state in (None, 'Z'):
                    break
                time.sleep(0.01)
            self.assertIn(state, (None, 'Z'), 'owned descendant survived cleanup')
            self.assertLess(time.monotonic() - started, 5)


if __name__ == '__main__':
    unittest.main()
