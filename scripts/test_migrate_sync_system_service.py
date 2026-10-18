"""Offline migration tests: fake managers, no sudo, services or host edits."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import types
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('migration', Path(__file__).with_name('migrate-sync-system-service.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

class MigrationTests(unittest.TestCase):
    def test_unit_preserves_confinement_and_operator_identity(self):
        source = (Path(__file__).parents[1] / 'dracon-sync.service').read_text()
        out = m.build_unit(source)
        for setting in ['ProtectSystem=strict', 'ProtectHome=read-only', 'PrivateTmp=true',
                        'CapabilityBoundingSet=', 'SystemCallFilter=@system-service',
                        'RestrictNamespaces=true', 'NoNewPrivileges=true']:
            self.assertIn(setting, out)
        for setting in ['User=dracon', 'Group=users', 'PrivateUsers=no', 'process.geteuid()!==1000',
                        'WantedBy=multi-user.target', "s.statSync('/').uid!==0"]:
            self.assertIn(setting, out)
        self.assertNotIn('%h', out)
        self.assertNotIn('%u', out)
        self.assertNotIn('User=root', out)

    def test_unexpected_execstart_rejected(self):
        with self.assertRaises(AssertionError): m.build_unit('[Service]\nExecStart=/other daemon\n')

    def simulation(self, fail_start=False, tamper=False):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary); home=root/'home'; config=home/'.config/systemd/user'; config.mkdir(parents=True)
            (home/'.dracon').mkdir()
            system=root/'system';system.mkdir()
            dest=root/'plan';dest.mkdir()
            original=config/'dracon-sync.service';original.write_text('original user unit\n')
            binary=home/'dracon-sync';binary.write_bytes(b'pinned executable')
            digest=m.sha(binary)
            for name in [m.UNIT,m.WATCH+'.service',m.WATCH+'.timer','dracon-sync-operator.environment',
                         '10-resource-headroom.conf','20-bridge-fixture.conf']:
                (dest/name).write_text('fixture for '+name+'\n')
            plan={'binarySha256':digest,'sourceHashes':{str(original):m.sha(original)},
                  'files':{p.name:m.sha(p) for p in dest.iterdir()}}
            (dest/'plan.json').write_text(json.dumps(plan))
            if tamper:(dest/m.UNIT).write_text('tampered')
            state={'user_active':True,'watchdog':True,'system_active':False};events=[]
            def completed(args, stdout=''):return subprocess.CompletedProcess(args,0,stdout,'')
            def fake_userctl(*args,check=True):
                events.append(('userctl',args))
                if args==('is-active','dracon-sync.service'):return completed(args,'active\n' if state['user_active'] else 'inactive\n')
                if args==('is-enabled','dracon-sync.service'):return completed(args,'masked\n' if original.is_symlink() else 'enabled\n')
                if args==('is-enabled','dracon-sync-watchdog.timer'):return completed(args,'enabled\n')
                if args==('disable','--now','dracon-sync.service'):state['user_active']=False
                if args==('enable','--now','dracon-sync.service'):state['user_active']=True
                if args==('disable','--now','dracon-sync-watchdog.timer'):state['watchdog']=False
                if args==('enable','--now','dracon-sync-watchdog.timer'):state['watchdog']=True
                return completed(args)
            def fake_run(args,check=True,seconds=45):
                args=list(map(str,args));events.append(('system',args))
                if args[1:]==['enable','--now',m.UNIT]:
                    assert not state['user_active'], 'duplicate schedulers'
                    if fail_start:raise subprocess.CalledProcessError(1,args)
                    state['system_active']=True
                if args[1:]==['disable','--now',m.UNIT]:state['system_active']=False
                if args[1:]==['is-active',m.UNIT]:return completed(args,'active\n')
                if '-p' in args and 'MainPID' in args:return completed(args,str(os.getpid())+'\n')
                return completed(args)
            real_sha=m.sha
            def fake_sha(path):
                return digest if str(path)==f'/proc/{os.getpid()}/exe' else real_sha(path)
            with patch.multiple(m,HOME=home,CONFIG=config,SYSTEM=system,BIN=binary,EXPECTED_BINARY=digest), \
                 patch.object(m.os,'geteuid',return_value=0), patch.object(m.os,'chown'), \
                 patch.object(m.pwd,'getpwnam',return_value=types.SimpleNamespace(pw_uid=1000,pw_gid=100)), \
                 patch.dict(os.environ,{'SUDO_USER':'dracon'}), patch.object(m,'run',side_effect=fake_run), \
                 patch.object(m,'userctl',side_effect=fake_userctl), patch.object(m,'user',return_value=completed([])), \
                 patch.object(m,'sha',side_effect=fake_sha):
                if fail_start or tamper:
                    with self.assertRaises((subprocess.CalledProcessError,AssertionError)):m.apply(dest)
                else:m.apply(dest)
            self.assertFalse((home/'.dracon/dracon-sync.maintenance-hold').exists())
            if fail_start or tamper:
                self.assertTrue(state['user_active']);self.assertTrue(state['watchdog'])
                self.assertEqual(original.read_text(),'original user unit\n')
                self.assertFalse((system/m.UNIT).exists())
            else:
                self.assertTrue(original.is_symlink());self.assertFalse(state['user_active'])
                self.assertTrue(state['system_active']);self.assertTrue((dest/'migration-receipt.json').exists())
            if tamper:self.assertEqual(events,[])

    def test_success_masks_old_before_starting_new(self):self.simulation()
    def test_failed_start_restores_original_and_watchdog(self):self.simulation(fail_start=True)
    def test_tampered_plan_refused_before_service_changes(self):self.simulation(tamper=True)

if __name__=='__main__':unittest.main()
