import unittest
from sync_timing import stage_timing


class StageTimingTests(unittest.TestCase):
    def test_independent_epoch_and_wall_clocks(self):
        lines = [
            'scheduler: pulse_start unix_ms=9000000000000',
            'scheduler: eligibility repo=/watch/b daemon_ms=3500 anchor_daemon_ms=1000 eligible=true',
            'scheduler: dispatch repo=/watch/b daemon_ms=3500 unix_ms=9000000004000',
            'scheduler: task_start repo=/watch/b unix_ms=9000000004001',
            'scheduler: stage_enter repo=/watch/b unix_ms=9000000004200',
            'scheduler: add_spawn repo=/watch/b unix_ms=9000000004250',
            'scheduler: dispatch repo=/watch/b daemon_ms=9500 unix_ms=9000000010000',
            'scheduler: add_spawn repo=/watch/b unix_ms=9000000010300',
        ]
        result = stage_timing(lines, '/watch/b')
        self.assertEqual(result['dispatch_after_quiet_ms'], 500)
        self.assertEqual(result['dispatch_to_worker_start_ms'], 1)
        self.assertEqual(result['worker_start_to_stage_enter_ms'], 199)
        self.assertEqual(result['stage_enter_to_add_spawn_ms'], 50)
        self.assertEqual(result['dispatch_to_add_spawn_ms'], 250)

    def test_missing_events_remain_unknown(self):
        self.assertEqual(stage_timing([], '/watch/b'), {})
        result = stage_timing([
            'scheduler: dispatch repo=/watch/b daemon_ms=3500 unix_ms=10000',
            'scheduler: add_spawn repo=/watch/b2 unix_ms=10020',
        ], '/watch/b')
        self.assertIsNone(result['dispatch_after_quiet_ms'])
        self.assertIsNone(result['dispatch_to_add_spawn_ms'])


if __name__ == '__main__':
    unittest.main()
