"""Offline migration tests: fake managers, no sudo, services or host edits."""
import contextlib
import importlib.util
import io
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

    def test_nixos_runtime_install_path(self):
        self.assertEqual(str(m.SYSTEM), '/run/systemd/system')
        source = (Path(__file__).parents[1] / 'dracon-sync.service').read_text()
        out = m.build_unit(source)
        self.assertIn('EnvironmentFile=-/run/systemd/system/dracon-sync-operator.environment', out)
        self.assertNotIn('/etc/systemd/system', out)

    def test_unexpected_execstart_rejected(self):
        with self.assertRaises(AssertionError): m.build_unit('[Service]\nExecStart=/other daemon\n')

    def simulation(self, fail_start=False, tamper=False, fail_stop=False, fail_resume=False, stale_marker=False, inflight_watchdog=False):
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
            state={'user_active':True,'watchdog':True,'system_active':False,'system_watchdog_active':False};events=[]
            def completed(args, stdout=''):return subprocess.CompletedProcess(args,0,stdout,'')
            def fake_userctl(*args,check=True):
                events.append(('userctl',args))
                if args==('is-active','dracon-sync.service'):return completed(args,'active\n' if state['user_active'] else 'inactive\n')
                if args==('is-enabled','dracon-sync.service'):return completed(args,'masked\n' if original.is_symlink() else 'enabled\n')
                if args==('is-enabled','dracon-sync-watchdog.timer'):return completed(args,'enabled\n')
                if args[0]=='show':
                    active = args[1]=='dracon-sync.service' and state['user_active']
                    return completed(args,'ActiveState='+('active' if active else 'inactive')+'\nMainPID=0\nControlPID=0\n')
                if args==('disable','--now','dracon-sync.service'):state['user_active']=False
                if args==('enable','--now','dracon-sync.service'):state['user_active']=True
                if args==('disable','--now','dracon-sync-watchdog.timer'):state['watchdog']=False
                if args==('enable','--now','dracon-sync-watchdog.timer'):state['watchdog']=True
                return completed(args)
            def fake_run(args,check=True,seconds=45,extra_env=None):
                args=list(map(str,args));events.append(('system',args))
                if args[1] in ('enable','disable'):
                    self.assertIn('--runtime',args,'system-manager enable/disable must never write /etc')
                if args[1:]==['enable','--runtime','--now',m.UNIT]:
                    assert not state['user_active'], 'duplicate schedulers'
                    state['system_active']=True
                    state['system_watchdog_active']=inflight_watchdog
                    if fail_start:raise subprocess.CalledProcessError(1,args)
                if args[1:]==['stop',m.WATCH+'.service']:state['system_watchdog_active']=False
                if args[1:]==['disable','--runtime','--now',m.UNIT]:
                    if fail_stop:raise subprocess.CalledProcessError(1,args)
                    state['system_active']=False
                if len(args)>2 and args[1]=='show' and '--value' not in args:
                    active = ((args[2]==m.UNIT and state['system_active']) or
                              (args[2]==m.WATCH+'.service' and state['system_watchdog_active']))
                    return completed(args,'ActiveState='+('active' if active else 'inactive')+'\nMainPID='+('123' if active else '0')+'\nControlPID=0\n')
                if args[1:]==['is-active',m.UNIT]:return completed(args,'active\n')
                if '-p' in args and 'MainPID' in args:return completed(args,str(os.getpid())+'\n')
                return completed(args)
            def fake_user(*args,check=True):
                events.append(('user',args))
                if args[-1]=='resume':
                    if fail_resume:
                        (home/'.dracon/dracon-sync.freeze').touch()
                        raise subprocess.CalledProcessError(1,args)
                    if stale_marker:(home/'.dracon/freeze').mkdir(exist_ok=True);(home/'.dracon/freeze/dracon-sync').touch()
                return completed(args)
            real_sha=m.sha
            def fake_sha(path):
                return digest if str(path)==f'/proc/{os.getpid()}/exe' else real_sha(path)
            with patch.multiple(m,HOME=home,CONFIG=config,SYSTEM=system,BIN=binary,EXPECTED_BINARY=digest), \
                 patch.object(m.os,'geteuid',return_value=0), patch.object(m.os,'chown'), \
                 patch.object(m.pwd,'getpwnam',return_value=types.SimpleNamespace(pw_uid=1000,pw_gid=100)), \
                 patch.dict(os.environ,{'SUDO_USER':'dracon'}), patch.object(m,'run',side_effect=fake_run), \
                 patch.object(m,'userctl',side_effect=fake_userctl), patch.object(m,'user',side_effect=fake_user), \
                 patch.object(m,'sha',side_effect=fake_sha):
                if fail_start or tamper or fail_resume or stale_marker:
                    with self.assertRaises((subprocess.CalledProcessError,AssertionError,RuntimeError)):m.apply(dest)
                else:m.apply(dest)
            if fail_stop or fail_resume or stale_marker:
                self.assertTrue((home/'.dracon/dracon-sync.maintenance-hold').exists())
                if fail_stop:
                    self.assertTrue(original.is_symlink())
                    self.assertFalse(state['user_active'])
                    self.assertTrue(state['system_active'])
                    self.assertNotIn(('userctl',('enable','--now','dracon-sync.service')),events)
                else:
                    self.assertTrue(state['user_active'])
                    self.assertFalse(state['system_active'])
                return
            self.assertFalse((home/'.dracon/dracon-sync.maintenance-hold').exists())
            if fail_start or tamper:
                self.assertTrue(state['user_active']);self.assertTrue(state['watchdog'])
                self.assertEqual(original.read_text(),'original user unit\n')
                self.assertFalse((system/m.UNIT).exists())
                self.assertFalse(state['system_watchdog_active'])
                if inflight_watchdog:
                    stop_watch=next(i for i,e in enumerate(events) if e[0]=='system' and e[1][1:]==['stop',m.WATCH+'.service'])
                    restart_user=events.index(('userctl',('enable','--now','dracon-sync.service')))
                    self.assertLess(stop_watch,restart_user)
            else:
                self.assertTrue(original.is_symlink());self.assertFalse(state['user_active'])
                self.assertTrue(state['system_active']);self.assertTrue((dest/'migration-receipt.json').exists())
            if tamper:self.assertEqual(events,[])

    def test_real_offline_runtime_enable_links_only_under_run(self):
        # Real systemctl against a disposable sysroot, never a live manager.
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);units=root/'run/systemd/system';units.mkdir(parents=True)
            (units/'proof.service').write_text('[Service]\nExecStart=/bin/true\n[Install]\nWantedBy=multi-user.target\n')
            subprocess.run(['systemctl','--root='+temporary,'enable','--runtime','proof.service'],
                           check=True,capture_output=True,text=True,timeout=10)
            self.assertTrue((units/'multi-user.target.wants/proof.service').is_symlink())
            self.assertFalse((root/'etc/systemd/system').exists())
            subprocess.run(['systemctl','--root='+temporary,'disable','--runtime','proof.service'],
                           check=True,capture_output=True,text=True,timeout=10)
            self.assertFalse((units/'multi-user.target.wants/proof.service').is_symlink())

    def test_post_start_reports_mask_failure_and_keeps_observations(self):
        with tempfile.TemporaryDirectory() as temporary:
            def control(*args):
                return types.SimpleNamespace(stdout='active\n' if args[0]=='is-active' else str(os.getpid())+'\n')
            with patch.object(m,'systemctl',side_effect=control), \
                 patch.object(m,'userctl',return_value=types.SimpleNamespace(stdout='enabled\n')), \
                 patch.object(m,'sha',return_value=m.EXPECTED_BINARY), patch.object(m.os,'chown'):
                with self.assertRaisesRegex(RuntimeError, 'old user unit is not masked'):
                    m.verify_running_scheduler(Path(temporary))
            self.assertEqual(json.loads((Path(temporary)/'post-start-checks.json').read_text())['oldUserUnitState'],'enabled')

    def test_post_start_reports_digest_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            def control(*args):
                return types.SimpleNamespace(stdout='active\n' if args[0]=='is-active' else str(os.getpid())+'\n')
            with patch.object(m,'systemctl',side_effect=control), \
                 patch.object(m,'userctl',return_value=types.SimpleNamespace(stdout='masked\n')), \
                 patch.object(m,'sha',return_value='0'*64), patch.object(m.os,'chown'):
                with self.assertRaisesRegex(RuntimeError, 'executable digest mismatch'):
                    m.verify_running_scheduler(Path(temporary))

    def test_success_masks_old_before_starting_new(self):self.simulation()
    def test_failed_start_restores_original_and_watchdog(self):self.simulation(fail_start=True)
    def test_tampered_plan_refused_before_service_changes(self):self.simulation(tamper=True)
    def test_failed_system_stop_never_restarts_user(self):self.simulation(fail_start=True,fail_stop=True)
    def test_inflight_system_watchdog_stopped_before_user_restart(self):self.simulation(fail_start=True,inflight_watchdog=True)
    def test_failed_resume_retains_hold(self):self.simulation(fail_resume=True)
    def test_resume_leaving_alternate_marker_retains_hold(self):self.simulation(stale_marker=True)
    def test_shutdown_confirmation_rejects_missing_process_metadata(self):
        with self.assertRaises(AssertionError):
            m.require_stopped(lambda *a:types.SimpleNamespace(stdout='ActiveState=inactive\n'), 'test.service')
    def test_dump_failure_persists_captured_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            dest = Path(temporary)
            error = subprocess.CalledProcessError(1, ['systemctl', 'enable', '--now', 'x'],
                                                   output='out-text', stderr='err-text')
            with patch.object(m.os, 'chown'):
                with contextlib.redirect_stderr(io.StringIO()) as captured:
                    m.dump_failure(dest, 'enable-now-x', error)
            log = (dest/'handover-failure.log').read_text()
            for text in ('enable-now-x', 'out-text', 'err-text'):
                self.assertIn(text, log)
                self.assertIn(text, captured.getvalue())

    def test_shutdown_confirmation_rejects_deactivating_service(self):
        with self.assertRaises(AssertionError):
            m.require_stopped(lambda *a:types.SimpleNamespace(stdout='ActiveState=deactivating\nMainPID=0\nControlPID=0\n'), 'test.service')

if __name__=='__main__':unittest.main()
