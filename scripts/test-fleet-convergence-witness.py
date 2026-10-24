#!/usr/bin/env python3
import unittest
from types import SimpleNamespace
from fleet_convergence_witness import select_witness


class HistoricalWitnessTests(unittest.TestCase):
    def setUp(self):
        self.before, self.created, self.later = (letter * 40 for letter in 'abc')
        self.expected = 'd' * 40
        self.history = [self.later, self.created]
        self.blobs = {self.created: self.expected, self.later: None}
        self.blob_errors = set()
        self.type_errors = set()
        self.witness = {'eligible_path': 'note.md', 'eligible_blob': self.expected,
                        'before_head': self.before, 'before_observed_at': 1000,
                        'file_mtime': 1000.5}
        self.events = [{'_PID': '42', '__REALTIME_TIMESTAMP': '1011000000',
                        'MESSAGE': '📝 committed 1 file(s) in /repo'}]

    def git(self, repo, *args):
        code, output = 0, ''
        if args[0] == 'log':
            output = '\n'.join(self.history)
        elif args[0] == 'rev-parse':
            commit = args[-1].split(':', 1)[0]
            output = self.blobs[commit] or args[-1]
            code = 128 if self.blobs[commit] is None or commit in self.blob_errors else 0
        elif args[0] == 'show':
            commit = args[-1]
            timestamp = 1010 if commit == self.created else 1050
            output = f'{commit}\n{self.before}\n{timestamp}\nDraconDev\ndracsharp@gmail.com\n'
        elif args[0] == 'cat-file':
            code, output = (128 if args[-1] in self.type_errors else 0), 'commit'
        else:
            raise AssertionError(args)
        return SimpleNamespace(returncode=code, stdout=output)

    def select(self):
        return select_witness('/repo', self.witness, self.events, 42, self.git,
                              lambda repo, before, after: before == self.before)

    def test_later_rename_does_not_erase_published_eligible_work(self):
        result = self.select()
        self.assertEqual(result['witness_commit'], self.created)
        self.assertEqual(result['actual_blob'], self.expected)
        self.assertTrue(result['object_and_advancement_verified'])
        self.assertEqual(result['candidate_commits_considered'], self.history)

    def test_newer_matching_copy_without_daemon_event_does_not_mask_valid_work(self):
        self.blobs[self.later] = self.expected
        self.assertEqual(self.select()['witness_commit'], self.created)

    def test_failed_blob_query_is_not_an_observed_object(self):
        self.blob_errors.add(self.created)
        result = self.select()
        self.assertEqual(result['witness_commit'], '')
        self.assertIsNone(result['actual_blob'])

    def test_failed_type_query_cannot_be_claimed_successful(self):
        self.type_errors.add(self.created)
        self.assertFalse(self.select()['object_and_advancement_verified'])

    def test_work_before_the_loaded_baseline_is_not_accepted(self):
        self.witness['before_observed_at'] = 1011
        self.assertEqual(self.select()['witness_commit'], '')

    def test_slow_hook_uses_recorded_finite_operation_budget(self):
        self.events[0]['__REALTIME_TIMESTAMP'] = '1041613204'
        self.assertEqual(self.select()['witness_commit'], '')
        self.witness['commit_event_budget_secs'] = 60
        self.assertEqual(self.select()['witness_commit'], self.created)

    def test_event_after_effective_budget_is_still_refused(self):
        self.witness['commit_event_budget_secs'] = 60
        self.events[0]['__REALTIME_TIMESTAMP'] = '1071000000'
        self.assertEqual(self.select()['witness_commit'], '')

    def test_a_different_daemon_pid_does_not_attest_the_commit(self):
        self.events[0]['_PID'] = '41'
        self.assertEqual(self.select()['witness_commit'], '')


if __name__ == '__main__':
    unittest.main()
