import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch, Mock

spec = importlib.util.spec_from_file_location('yuzu_play', Path(__file__).resolve().parents[1] / 'scripts/yuzu_play.py')
launcher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launcher)

class DiagnosticLaunchTest(unittest.TestCase):
    def check_mode(self, diagnostic):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            build = root / 'build'
            work = build / 'work'
            work.mkdir(parents=True)
            namebase = 'ssbu_diagnostic' if diagnostic else 'ssbu_native'
            for name in (namebase + '.exe', namebase + '.pdb', namebase + '.map', 'ssbu_game_eh_o1.dll'):
                (work / name).write_bytes(name.encode())
            userdata = root / 'existing-save'
            for name in ('save', 'cache', 'save.work.123'):
                (userdata / name).mkdir(parents=True)
                (userdata / name / 'state.bin').write_bytes(name.encode())
            (build / 'config').mkdir()
            (build / 'config/graphics.ini').write_text('renderer=vulkan')
            (build / 'state.json').write_text(json.dumps({'dump': {'romfs': str(root / 'romfs')}}))
            args = ['yuzu_play.py', '--nickname', 'udon', '--userdata', str(userdata)] + (['--diagnostic'] if diagnostic else [])
            with patch.object(launcher, 'ROOT', root), patch('sys.argv', args), patch.object(launcher.subprocess, 'Popen', return_value=Mock(pid=1234)) as spawn, patch.dict(os.environ, {'SSBU_DEBUG_END': '1', 'SSBU_PLAY': 'unrelated.rec'}):
                self.assertEqual(launcher.main(), 0)
            env = spawn.call_args.kwargs['env']
            self.assertEqual(env['SSBU_NICKNAME'], 'udon')
            self.assertNotIn('SSBU_DEBUG_END', env)
            self.assertNotIn('SSBU_PLAY', env)
            self.assertEqual(env['SSBU_USERDATA'], str(userdata))
            self.assertEqual((userdata / 'save/state.bin').read_bytes(), b'save')
            if diagnostic:
                folder = Path(env['SSBU_REPORT_DIR'])
                self.assertEqual(env['SSBU_MINIDUMP'], 'full')
                self.assertEqual(env['SSBU_SHOW'], '1')
                self.assertEqual(Path(env['SSBU_PAD_TRACE']), folder / 'pads.csv')
                self.assertEqual(Path(env['SSBU_REC']).parent, folder)
                self.assertEqual(Path(env['SSBU_TRACE']).parent, folder)
                self.assertEqual(Path(env['SSBU_NET_UI_LOG']), folder / 'game.log')
                self.assertEqual((folder / 'userdata_before/save/state.bin').read_bytes(), b'save')
                self.assertFalse((folder / 'userdata_before/save.work.123').exists())
                manifest = json.loads((folder / 'manifest.json').read_text())
                self.assertEqual(len(manifest['binary_sha256']), 4)
                self.assertEqual(json.loads((folder / 'process.json').read_text())['pid'], 1234)
                (work / (namebase + '.exe')).write_bytes(b'changed')
                self.assertEqual((folder / ('binaries/' + namebase + '.exe')).read_bytes(), (namebase + '.exe').encode())
            else:
                self.assertEqual(env['SSBU_MINIDUMP'], '1')
                self.assertNotIn('SSBU_REPORT_DIR', env)
                self.assertFalse(list((build / 'logs').glob('yuzu-diagnostic-*')))
    def test_diagnostic_isolated_evidence(self): self.check_mode(True)
    def test_normal_launch_keeps_default_mode(self): self.check_mode(False)

if __name__ == '__main__': unittest.main()
