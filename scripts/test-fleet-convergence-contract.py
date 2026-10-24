#!/usr/bin/env python3
import copy
import unittest
from fleet_convergence_contract import destination, prior_observations, validate_fleet


def evidence():
    initial = [{'repo': '/fleet/a', 'push_to_remotes': ['github']},
               {'repo': '/fleet/b', 'push_to_remotes': ['github']}]
    rows = []
    for name in ('a', 'b'):
        identity = f'github.com/owner/{name}'
        remote = {'url_without_credentials': f'git@github.com:Owner/{name}.git',
                  'query_succeeded': True, 'object_type': 'commit', 'branch_ref': '1' * 40}
        rows.append({'repo': f'/fleet/{name}', 'remotes': {'origin': remote, 'mirror': copy.deepcopy(remote)},
                     'intended_destinations': [
                         {'logical': logical, 'resolution': 'configured-destination',
                          'destination': identity, 'aliases': ['origin', 'mirror']}
                         for logical in ('origin', 'github')]})
    return initial, copy.deepcopy(rows), copy.deepcopy(rows), copy.deepcopy(rows)


class ContractTests(unittest.TestCase):
    def test_complete_inventory_is_accepted(self):
        validate_fleet(*evidence())

    def test_empty_or_missing_inventory_never_proves_a_subset(self):
        for index in range(4):
            for empty in (False, True):
                with self.subTest(index=index, empty=empty):
                    data = list(evidence())
                    data[index] = [] if empty else data[index][:-1]
                    with self.assertRaises(ValueError):
                        validate_fleet(*data)

    def test_duplicate_repository_is_rejected_in_every_input(self):
        for index in range(4):
            with self.subTest(index=index):
                data = list(evidence())
                data[index].append(copy.deepcopy(data[index][0]))
                with self.assertRaises(ValueError):
                    validate_fleet(*data)

    def test_missing_or_duplicate_logical_destination_is_rejected(self):
        for index in (1, 2, 3):
            for duplicate in (False, True):
                data = list(evidence())
                targets = data[index][0]['intended_destinations']
                targets.append(copy.deepcopy(targets[0])) if duplicate else targets.pop()
                with self.assertRaises(ValueError):
                    validate_fleet(*data)

    def test_alias_retarget_cannot_inherit_same_commit_ancestry(self):
        initial, witnesses, before, after = evidence()
        for target in after[0]['intended_destinations']:
            target['destination'] = 'github.com/other/repository'
        for remote in after[0]['remotes'].values():
            remote['url_without_credentials'] = 'https://github.com/other/repository.git'
        # Object IDs deliberately remain identical: ancestry cannot excuse
        # substituting a different intended endpoint.
        with self.assertRaises(ValueError):
            validate_fleet(initial, witnesses, before, after)

    def test_stale_descriptor_cannot_hide_retargeted_alias(self):
        data = list(evidence())
        data[3][0]['remotes']['mirror']['url_without_credentials'] = 'https://github.com/other/repo.git'
        with self.assertRaises(ValueError):
            validate_fleet(*data)

    def test_transport_spelling_and_extra_alias_rename_are_harmless(self):
        initial, witnesses, before, after = evidence()
        row = after[0]
        row['remotes']['renamed'] = row['remotes'].pop('mirror')
        for remote in row['remotes'].values():
            remote['url_without_credentials'] = 'https://github.com/OWNER/A.git'
        for target in row['intended_destinations']:
            target['aliases'] = ['origin', 'renamed']
        validate_fleet(initial, witnesses, before, after)
        priors = prior_observations(before[0], destination(row['remotes']['renamed']['url_without_credentials']))
        self.assertEqual({name for name, _ in priors}, {'origin', 'mirror'})

    def test_failed_or_unverified_prior_query_never_becomes_continuity(self):
        row = evidence()[2][0]
        row['remotes']['origin']['query_succeeded'] = False
        row['remotes']['mirror']['object_type'] = None
        self.assertEqual(prior_observations(row, 'github.com/owner/a'), [])


if __name__ == '__main__':
    unittest.main()
